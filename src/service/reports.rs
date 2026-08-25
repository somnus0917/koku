//! 可打印报表生成：把年度汇总排版为自包含 PDF。

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, ObjectId, Stream, StringFormat};
use rust_decimal::prelude::ToPrimitive;

use crate::domain::{CashFlowItem, YearlySummary};
use crate::error::{KokuError, Result};

const PAGE_WIDTH: f32 = 595.0;
const PAGE_HEIGHT: f32 = 842.0;

/// 生成 A4 年度汇总 PDF。中文使用 PDF 预定义的 GB 字体映射，避免服务端依赖字体文件。
pub(crate) fn yearly_summary_pdf(summary: &YearlySummary) -> Result<Vec<u8>> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let latin_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    let latin_bold_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica-Bold",
        "Encoding" => "WinAnsiEncoding",
    });
    let cid_font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType0",
        "BaseFont" => "STSong-Light",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("GB1"),
            "Supplement" => 4,
        },
        "DW" => 1000,
    });
    let cjk_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "STSong-Light",
        "Encoding" => "UniGB-UCS2-H",
        "DescendantFonts" => vec![Object::Reference(cid_font_id)],
    });
    let fonts = PdfFonts {
        latin: latin_id,
        latin_bold: latin_bold_id,
        cjk: cjk_id,
    };

    let mut page_ids = Vec::new();
    add_page(
        &mut document,
        pages_id,
        fonts,
        overview_page(summary),
        &mut page_ids,
    )?;

    let row_limit = 28;
    let category_pages = summary
        .income_sources
        .len()
        .max(summary.expense_destinations.len())
        .div_ceil(row_limit)
        .max(1);
    for page_index in 0..category_pages {
        let start = page_index * row_limit;
        let income = summary
            .income_sources
            .get(start..(start + row_limit).min(summary.income_sources.len()))
            .unwrap_or_default();
        let expenses = summary
            .expense_destinations
            .get(start..(start + row_limit).min(summary.expense_destinations.len()))
            .unwrap_or_default();
        add_page(
            &mut document,
            pages_id,
            fonts,
            categories_page(summary, income, expenses, page_index + 1, category_pages),
            &mut page_ids,
        )?;
    }

    document.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>(),
            "Count" => page_ids.len() as i64,
        }
        .into(),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    let mut title = Vec::new();
    lopdf::encode_utf16_be(&format!("Koku {} 年度汇总", summary.year), &mut title);
    let info_id = document.add_object(dictionary! {
        "Title" => Object::String(title, StringFormat::Hexadecimal),
        "Creator" => Object::string_literal("Koku"),
    });
    document.trailer.set("Root", catalog_id);
    document.trailer.set("Info", info_id);
    document.compress();
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).map_err(|error| {
        KokuError::Io(std::io::Error::other(format!(
            "PDF generation failed: {error}"
        )))
    })?;
    Ok(bytes)
}

#[derive(Clone, Copy)]
struct PdfFonts {
    latin: ObjectId,
    latin_bold: ObjectId,
    cjk: ObjectId,
}

fn add_page(
    document: &mut Document,
    pages_id: ObjectId,
    fonts: PdfFonts,
    content: Content,
    page_ids: &mut Vec<ObjectId>,
) -> Result<()> {
    let encoded = content.encode().map_err(|error| {
        KokuError::Io(std::io::Error::other(format!(
            "PDF encoding failed: {error}"
        )))
    })?;
    let content_id = document.add_object(Stream::new(dictionary! {}, encoded));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), PAGE_WIDTH.into(), PAGE_HEIGHT.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "F1" => fonts.latin,
                "F2" => fonts.latin_bold,
                "FC" => fonts.cjk,
            },
        },
        "Contents" => content_id,
    });
    page_ids.push(page_id);
    Ok(())
}

fn overview_page(summary: &YearlySummary) -> Content {
    let mut page = PdfPage::new();
    page.fill_rgb(0.965, 0.961, 0.941);
    page.rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT, true);
    page.fill_rgb(0.12, 0.13, 0.12);
    page.text_cjk(42.0, 790.0, 26.0, &format!("{} 年度财务汇总", summary.year));
    page.text_latin(42.0, 770.0, 9.0, "KOKU  /  YEARLY FINANCIAL REPORT");
    page.text_latin(494.0, 790.0, 10.0, &summary.currency);

    metric_card(
        &mut page,
        42.0,
        696.0,
        "全年收入",
        &money(summary.total_income, &summary.currency),
        (0.15, 0.36, 0.29),
    );
    metric_card(
        &mut page,
        216.0,
        696.0,
        "全年支出",
        &money(summary.total_expense, &summary.currency),
        (0.73, 0.35, 0.19),
    );
    metric_card(
        &mut page,
        390.0,
        696.0,
        "全年结余",
        &money(summary.net, &summary.currency),
        (0.22, 0.32, 0.52),
    );

    section_title(&mut page, 42.0, 652.0, "逐月收支趋势", "MONTHLY TREND");
    draw_chart(&mut page, summary, 42.0, 468.0, 511.0, 152.0);
    section_title(&mut page, 42.0, 425.0, "月度明细", "MONTHLY DETAIL");
    monthly_table(&mut page, summary, 42.0, 398.0);
    footer(&mut page, 1);
    page.finish()
}

fn categories_page(
    summary: &YearlySummary,
    income: &[CashFlowItem],
    expenses: &[CashFlowItem],
    index: usize,
    total: usize,
) -> Content {
    let mut page = PdfPage::new();
    page.fill_rgb(0.965, 0.961, 0.941);
    page.rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT, true);
    page.fill_rgb(0.12, 0.13, 0.12);
    page.text_cjk(42.0, 790.0, 24.0, "分类汇总");
    page.text_latin(
        42.0,
        770.0,
        9.0,
        &format!(
            "{}  /  {}  /  CATEGORY BREAKDOWN",
            summary.year, summary.currency
        ),
    );
    if total > 1 {
        page.text_latin(510.0, 790.0, 9.0, &format!("{index}/{total}"));
    }
    category_column(
        &mut page,
        (42.0, 726.0, 245.0),
        "收入来源",
        income,
        &summary.currency,
        (0.15, 0.36, 0.29),
    );
    category_column(
        &mut page,
        (308.0, 726.0, 245.0),
        "支出去向",
        expenses,
        &summary.currency,
        (0.73, 0.35, 0.19),
    );
    footer(&mut page, index + 1);
    page.finish()
}

fn metric_card(
    page: &mut PdfPage,
    x: f32,
    y: f32,
    label: &str,
    value: &str,
    tone: (f32, f32, f32),
) {
    page.fill_rgb(1.0, 1.0, 0.99);
    page.rect(x, y, 163.0, 54.0, true);
    page.fill_rgb(tone.0, tone.1, tone.2);
    page.rect(x, y, 4.0, 54.0, true);
    page.text_cjk(x + 14.0, y + 34.0, 9.0, label);
    page.text_latin_bold(x + 14.0, y + 14.0, 12.0, value);
}

fn section_title(page: &mut PdfPage, x: f32, y: f32, title: &str, eyebrow: &str) {
    page.fill_rgb(0.12, 0.13, 0.12);
    page.text_cjk(x, y, 14.0, title);
    page.fill_rgb(0.48, 0.5, 0.46);
    page.text_latin(x + 112.0, y + 1.0, 8.0, eyebrow);
}

fn draw_chart(
    page: &mut PdfPage,
    summary: &YearlySummary,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    page.fill_rgb(1.0, 1.0, 0.99);
    page.rect(x, y, width, height, true);
    let max = summary
        .months
        .iter()
        .flat_map(|month| [month.total_income, month.total_expense])
        .filter_map(|value| value.to_f32())
        .fold(1.0_f32, f32::max);
    let chart_x = x + 18.0;
    let chart_y = y + 24.0;
    let chart_height = height - 40.0;
    let slot = (width - 34.0) / 12.0;
    for line in 0..=4 {
        let line_y = chart_y + chart_height * line as f32 / 4.0;
        page.stroke_rgb(0.87, 0.87, 0.83);
        page.line(chart_x, line_y, x + width - 12.0, line_y, 0.35);
    }
    for (index, month) in summary.months.iter().enumerate() {
        let income = month.total_income.to_f32().unwrap_or_default().max(0.0);
        let expense = month.total_expense.to_f32().unwrap_or_default().max(0.0);
        let center = chart_x + slot * index as f32 + slot / 2.0;
        let income_height = chart_height * income / max;
        let expense_height = chart_height * expense / max;
        page.fill_rgb(0.15, 0.36, 0.29);
        page.rect(center - 8.0, chart_y, 6.0, income_height, true);
        page.fill_rgb(0.87, 0.48, 0.29);
        page.rect(center + 1.0, chart_y, 6.0, expense_height, true);
        page.fill_rgb(0.35, 0.36, 0.34);
        page.text_latin(center - 4.0, y + 8.0, 7.0, &(index + 1).to_string());
    }
    page.fill_rgb(0.15, 0.36, 0.29);
    page.rect(x + width - 110.0, y + height - 12.0, 7.0, 3.0, true);
    page.text_cjk(x + width - 99.0, y + height - 15.0, 7.0, "收入");
    page.fill_rgb(0.87, 0.48, 0.29);
    page.rect(x + width - 59.0, y + height - 12.0, 7.0, 3.0, true);
    page.text_cjk(x + width - 48.0, y + height - 15.0, 7.0, "支出");
}

fn monthly_table(page: &mut PdfPage, summary: &YearlySummary, x: f32, y: f32) {
    page.fill_rgb(0.36, 0.37, 0.34);
    page.text_cjk(x + 8.0, y, 8.0, "月份");
    page.text_cjk(x + 96.0, y, 8.0, "收入");
    page.text_cjk(x + 258.0, y, 8.0, "支出");
    page.text_cjk(x + 420.0, y, 8.0, "结余");
    for (index, month) in summary.months.iter().enumerate() {
        let row_y = y - 20.0 - index as f32 * 18.0;
        if index % 2 == 0 {
            page.fill_rgb(1.0, 1.0, 0.99);
            page.rect(x, row_y - 5.0, 511.0, 16.0, true);
        }
        page.fill_rgb(0.17, 0.18, 0.17);
        page.text_cjk(x + 8.0, row_y, 8.0, &format!("{} 月", month.month));
        page.text_latin(x + 96.0, row_y, 8.0, &amount(month.total_income));
        page.text_latin(x + 258.0, row_y, 8.0, &amount(month.total_expense));
        page.text_latin(x + 420.0, row_y, 8.0, &amount(month.net));
    }
}

fn category_column(
    page: &mut PdfPage,
    bounds: (f32, f32, f32),
    title: &str,
    items: &[CashFlowItem],
    currency: &str,
    tone: (f32, f32, f32),
) {
    let (x, y, width) = bounds;
    page.fill_rgb(tone.0, tone.1, tone.2);
    page.rect(x, y, width, 4.0, true);
    page.text_cjk(x, y - 24.0, 14.0, title);
    page.fill_rgb(0.48, 0.5, 0.46);
    page.text_latin(x + width - 40.0, y - 22.0, 8.0, currency);
    if items.is_empty() {
        page.text_cjk(x, y - 56.0, 9.0, "暂无数据");
        return;
    }
    for (index, item) in items.iter().enumerate() {
        let row_y = y - 52.0 - index as f32 * 23.0;
        if index % 2 == 0 {
            page.fill_rgb(1.0, 1.0, 0.99);
            page.rect(x, row_y - 6.0, width, 19.0, true);
        }
        page.fill_rgb(0.17, 0.18, 0.17);
        page.text_cjk(
            x + 7.0,
            row_y,
            8.0,
            &truncate_chars(&item.category_name, 11),
        );
        page.fill_rgb(0.48, 0.5, 0.46);
        page.text_latin(
            x + 112.0,
            row_y,
            7.0,
            &format!("{}%", item.percentage.round_dp(1)),
        );
        page.fill_rgb(0.17, 0.18, 0.17);
        page.text_latin(x + 160.0, row_y, 7.5, &amount(item.amount));
    }
}

fn footer(page: &mut PdfPage, number: usize) {
    page.stroke_rgb(0.8, 0.8, 0.76);
    page.line(42.0, 35.0, 553.0, 35.0, 0.5);
    page.fill_rgb(0.48, 0.5, 0.46);
    page.text_latin(42.0, 20.0, 7.0, "Generated by Koku");
    page.text_latin(535.0, 20.0, 7.0, &number.to_string());
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn amount(value: rust_decimal::Decimal) -> String {
    let normalized = value.round_dp(2).normalize().to_string();
    let (sign, digits) = normalized
        .strip_prefix('-')
        .map_or(("", normalized.as_str()), |rest| ("-", rest));
    let (integer, fraction) = digits.split_once('.').map_or((digits, ""), |parts| parts);
    let mut grouped = String::new();
    for (index, character) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(character);
    }
    let integer = grouped.chars().rev().collect::<String>();
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fraction}")
    }
}

fn money(value: rust_decimal::Decimal, currency: &str) -> String {
    format!("{} {currency}", amount(value))
}

struct PdfPage {
    operations: Vec<Operation>,
}

impl PdfPage {
    fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    fn finish(self) -> Content {
        Content {
            operations: self.operations,
        }
    }

    fn fill_rgb(&mut self, red: f32, green: f32, blue: f32) {
        self.op("rg", vec![red.into(), green.into(), blue.into()]);
    }

    fn stroke_rgb(&mut self, red: f32, green: f32, blue: f32) {
        self.op("RG", vec![red.into(), green.into(), blue.into()]);
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: bool) {
        self.op("re", vec![x.into(), y.into(), width.into(), height.into()]);
        self.op(if fill { "f" } else { "S" }, vec![]);
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        self.op("w", vec![width.into()]);
        self.op("m", vec![x1.into(), y1.into()]);
        self.op("l", vec![x2.into(), y2.into()]);
        self.op("S", vec![]);
    }

    fn text_latin(&mut self, x: f32, y: f32, size: f32, value: &str) {
        self.text("F1", x, y, size, Object::string_literal(value));
    }

    fn text_latin_bold(&mut self, x: f32, y: f32, size: f32, value: &str) {
        self.text("F2", x, y, size, Object::string_literal(value));
    }

    fn text_cjk(&mut self, x: f32, y: f32, size: f32, value: &str) {
        let bytes = value
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        self.text(
            "FC",
            x,
            y,
            size,
            Object::String(bytes, StringFormat::Hexadecimal),
        );
    }

    fn text(&mut self, font: &str, x: f32, y: f32, size: f32, value: Object) {
        self.op("BT", vec![]);
        self.op(
            "Tf",
            vec![Object::Name(font.as_bytes().to_vec()), size.into()],
        );
        self.op("Td", vec![x.into(), y.into()]);
        self.op("Tj", vec![value]);
        self.op("ET", vec![]);
    }

    fn op(&mut self, operator: &str, operands: Vec<Object>) {
        self.operations.push(Operation::new(operator, operands));
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::*;
    use crate::domain::MonthlyTrendPoint;

    fn sample_summary() -> YearlySummary {
        YearlySummary {
            year: 2026,
            currency: "CNY".to_owned(),
            total_income: Decimal::from(180_000),
            total_expense: Decimal::from(112_500),
            net: Decimal::from(67_500),
            months: (1..=12)
                .map(|month| MonthlyTrendPoint {
                    year: 2026,
                    month,
                    total_income: Decimal::from(12_000 + month * 500),
                    total_expense: Decimal::from(7_000 + month * 375),
                    net: Decimal::from(5_000 + month * 125),
                })
                .collect(),
            income_sources: vec![
                CashFlowItem {
                    category_id: 1,
                    category_name: "工资收入".to_owned(),
                    amount: Decimal::from(168_000),
                    percentage: Decimal::from_str("93.3").unwrap(),
                },
                CashFlowItem {
                    category_id: 2,
                    category_name: "利息收入".to_owned(),
                    amount: Decimal::from(12_000),
                    percentage: Decimal::from_str("6.7").unwrap(),
                },
            ],
            expense_destinations: vec![
                CashFlowItem {
                    category_id: 3,
                    category_name: "住房".to_owned(),
                    amount: Decimal::from(60_000),
                    percentage: Decimal::from_str("53.3").unwrap(),
                },
                CashFlowItem {
                    category_id: 4,
                    category_name: "餐饮与日常消费".to_owned(),
                    amount: Decimal::from(52_500),
                    percentage: Decimal::from_str("46.7").unwrap(),
                },
            ],
        }
    }

    #[test]
    fn yearly_report_is_a_parseable_two_page_pdf() {
        let bytes = yearly_summary_pdf(&sample_summary()).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.5"));
        let document = Document::load_mem(&bytes).unwrap();
        assert_eq!(document.get_pages().len(), 2);
        if let Ok(path) = std::env::var("KOKU_PDF_SAMPLE_PATH") {
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn amount_uses_grouping_without_forcing_fractional_zeroes() {
        assert_eq!(
            amount(Decimal::from_str("1234567.80").unwrap()),
            "1,234,567.8"
        );
        assert_eq!(amount(Decimal::from(-42)), "-42");
    }
}
