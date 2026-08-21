//! 银行账单导入解析：CSV / QIF / OFX（SGML 或 XML 均可）。
//!
//! 解析只负责「格式识别 + 行级转换」，不写库；业务写入（账户/分类/去重/余额联动）
//! 在 `service::import` 层完成。三种格式统一输出 [`ImportRow`]：
//! `amount` 带符号（负数 = 支出，正数 = 收入），`date` 为自然日。
//!
//! - CSV：自动识别两种布局——Koku 自身导出的列（`occurred_at`/`settled_amount`/
//!   `kind`），以及常见银行流水列（日期/金额/备注，支持中英文列名别名）。
//! - QIF：`!Type:Bank` / `!Type:CCard` / `!Type:Cash` 段的 `D/T/P/M` 记录。
//! - OFX：扫描 `<STMTTRN>` 块（对 OFX 1.x SGML 与 2.x XML 同样有效），
//!   提取 `DTPOSTED`/`TRNAMT`/`NAME`/`MEMO`/`TRNTYPE`。

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{KokuError, Result};

/// 导入在进入格式解析前的资源限制；配合 API body 上限，避免单行字段或超长账单
/// 让解析结果/错误列表无限增长。
pub const MAX_IMPORT_LINES: usize = 100_000;
pub const MAX_IMPORT_LINE_BYTES: usize = 16 * 1024;

/// 支持的导入格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    Csv,
    Qif,
    Ofx,
}

impl ImportFormat {
    pub fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "qif" => Ok(Self::Qif),
            "ofx" => Ok(Self::Ofx),
            other => Err(KokuError::InvalidInput(format!(
                "unsupported import format: {other} (expected csv, qif, or ofx)"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Qif => "qif",
            Self::Ofx => "ofx",
        }
    }

    /// 按文件名扩展名猜测格式（用于 `auto`）。
    pub fn from_filename(filename: &str) -> Option<Self> {
        let lower = filename.to_ascii_lowercase();
        if lower.ends_with(".qif") {
            Some(Self::Qif)
        } else if lower.ends_with(".ofx") || lower.ends_with(".qfx") {
            Some(Self::Ofx)
        } else {
            None
        }
    }
}

/// 解析后的一行流水。
#[derive(Debug, Clone, Serialize)]
pub struct ImportRow {
    /// 源文件行号（CSV 数据行、QIF 记录序号、OFX 交易序号）；未知为 0。
    pub line: u32,
    pub date: NaiveDate,
    /// 带符号金额：负数 = 支出，正数 = 收入。
    pub amount: Decimal,
    #[serde(default)]
    pub note: String,
    /// 流水原始币种；缺省时使用账户币种。
    #[serde(default)]
    pub currency: Option<String>,
    /// Koku 导出格式特有：分类名（按 kind 匹配）。
    #[serde(default)]
    pub category_name: Option<String>,
    /// Koku 导出格式特有：结算金额（跨币种时直接使用）。
    #[serde(default)]
    pub settled_amount: Option<Decimal>,
    /// Koku 导出格式特有：商户名称（导入时恢复 Payee，不视为新的人工确认）。
    #[serde(default)]
    pub payee_name: Option<String>,
    /// 导入时的原始流水描述（QIF/OFX/通用银行 CSV 生成；Koku 导出 CSV 亦恢复）。
    #[serde(default)]
    pub raw_description: Option<String>,
    /// 外部唯一流水 ID（OFX `FITID`、通用 CSV 流水号列）：存在时优先用于去重。
    #[serde(default)]
    pub external_id: Option<String>,
}

/// 行级解析问题（不中断整份文件）。
#[derive(Debug, Clone, Serialize)]
pub struct ParseIssue {
    pub line: u32,
    pub message: String,
}

type ParseOutcome = (Vec<ImportRow>, Vec<ParseIssue>);

/// 按格式分发解析。
pub fn parse(input: &str, format: ImportFormat) -> Result<ParseOutcome> {
    let mut lines = 0_usize;
    for line in input.lines() {
        lines += 1;
        if lines > MAX_IMPORT_LINES {
            return Err(KokuError::InvalidInput(format!(
                "import exceeds the {MAX_IMPORT_LINES} line limit"
            )));
        }
        if line.len() > MAX_IMPORT_LINE_BYTES {
            return Err(KokuError::InvalidInput(format!(
                "import contains a line larger than {MAX_IMPORT_LINE_BYTES} bytes"
            )));
        }
    }
    match format {
        ImportFormat::Csv => parse_csv(input),
        ImportFormat::Qif => parse_qif(input),
        ImportFormat::Ofx => parse_ofx(input),
    }
}

/// 轻量格式嗅探：QIF 以 `!Type:` 开头，OFX 含 `<STMTTRN>`，其余视为 CSV。
pub fn sniff_format(input: &str) -> ImportFormat {
    let head = &input[..input.len().min(4096)];
    if head.to_ascii_uppercase().contains("<STMTTRN>") {
        ImportFormat::Ofx
    } else if head.lines().any(|line| line.trim_start().starts_with('!')) {
        ImportFormat::Qif
    } else {
        ImportFormat::Csv
    }
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

/// CSV 列名别名（小写、去空格后匹配）。
const DATE_ALIASES: &[&str] = &[
    "date",
    "日期",
    "交易日期",
    "记账日期",
    "时间",
    "time",
    "posted",
    "posteddate",
    "交易时间",
];
const AMOUNT_ALIASES: &[&str] = &[
    "amount",
    "金额",
    "交易金额",
    "入账金额",
    "金额(元)",
    "金额（元）",
    "withdrawal",
    "deposit",
    "支出金额",
    "收入金额",
    "发生额",
    "交易额",
];
const NOTE_ALIASES: &[&str] = &[
    "note",
    "description",
    "desc",
    "备注",
    "摘要",
    "memo",
    "name",
    "交易备注",
    "用途",
    "交易摘要",
    "商品说明",
];

/// 通用银行 CSV 中的「商户」列名（作为商户识别原始文本，区别于用户备注）。
const PAYEE_ALIASES: &[&str] = &[
    "payee",
    "merchant",
    "counterparty",
    "商户",
    "商户名称",
    "交易对方",
    "对方户名",
    "收款方",
    "付款方",
];

/// 通用银行 CSV 中的「外部唯一流水号」列名（优先用于导入去重）。
/// 刻意不包含普通 `id`：避免把行号/自增 id 误当流水号。
const EXTERNAL_ID_ALIASES: &[&str] = &[
    "transaction_id",
    "reference",
    "reference_number",
    "流水号",
    "交易流水号",
    "交易编号",
    "参考号",
];
const CURRENCY_ALIASES: &[&str] = &["currency", "币种", "货币", "交易币种"];
const TYPE_ALIASES: &[&str] = &[
    "type",
    "kind",
    "类型",
    "收支类型",
    "direction",
    "借贷标志",
    "收支",
];

fn alias_index(headers: &[String], aliases: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        let normalized = header.trim().to_lowercase().replace(' ', "");
        aliases
            .iter()
            .any(|alias| normalized == *alias || normalized.contains(alias))
    })
}

pub fn parse_csv(input: &str) -> Result<ParseOutcome> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(input.as_bytes());
    let headers: Vec<String> = reader
        .headers()?
        .iter()
        .map(|header| header.trim().to_lowercase())
        .collect();
    if headers.iter().any(|header| header.contains("occurred_at")) {
        parse_koku_export_csv(reader, &headers)
    } else {
        parse_generic_csv(reader, &headers)
    }
}

/// Koku 自身导出格式（id,kind,account,target_account,category,amount,currency,
/// settled_amount,occurred_at,note,voided_at）：支持导出→编辑→再导入的往返。
fn parse_koku_export_csv(
    mut reader: csv::Reader<&[u8]>,
    headers: &[String],
) -> Result<ParseOutcome> {
    let col = |name: &str| headers.iter().position(|header| header == name);
    let kind_col = col("kind");
    let amount_col = col("amount");
    let currency_col = col("currency");
    let settled_col = col("settled_amount");
    let date_col = col("occurred_at");
    let note_col = col("note");
    let category_col = col("category");
    let payee_col = col("payee");
    let raw_col = col("raw_description");
    if kind_col.is_none() || amount_col.is_none() || date_col.is_none() {
        return Err(KokuError::InvalidInput(
            "koku export CSV is missing required columns (kind/amount/occurred_at)".to_owned(),
        ));
    }

    let mut rows = Vec::new();
    let mut issues = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let line = index as u32 + 2; // 表头占第 1 行
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                issues.push(ParseIssue {
                    line,
                    message: format!("无法解析该行: {error}"),
                });
                continue;
            }
        };
        let get = |column: Option<usize>| column.and_then(|index| record.get(index)).unwrap_or("");
        let kind = get(kind_col).trim();
        if !matches!(kind, "income" | "expense") {
            issues.push(ParseIssue {
                line,
                message: format!("跳过非收支流水（kind={kind}）"),
            });
            continue;
        }
        let Some(amount) = parse_amount(get(amount_col)) else {
            issues.push(ParseIssue {
                line,
                message: format!("无效金额: {}", get(amount_col)),
            });
            continue;
        };
        let Some(date) = parse_date(get(date_col)) else {
            issues.push(ParseIssue {
                line,
                message: format!("无效日期: {}", get(date_col)),
            });
            continue;
        };
        // kind 为 income/expense 时导出金额为正，转为带符号金额。
        let signed = if kind == "expense" { -amount } else { amount };
        let currency = non_empty(get(currency_col)).map(str::to_owned);
        let settled = parse_amount(get(settled_col));
        rows.push(ImportRow {
            line,
            date,
            amount: signed,
            note: non_empty(get(note_col)).unwrap_or("").to_owned(),
            currency,
            category_name: non_empty(get(category_col)).map(str::to_owned),
            settled_amount: settled,
            payee_name: non_empty(get(payee_col)).map(str::to_owned),
            raw_description: non_empty(get(raw_col)).map(str::to_owned),
            external_id: None,
        });
    }
    Ok((rows, issues))
}

/// 通用银行流水 CSV：自动识别日期/金额/备注/币种/收支类型列。
fn parse_generic_csv(mut reader: csv::Reader<&[u8]>, headers: &[String]) -> Result<ParseOutcome> {
    let date_col = alias_index(headers, DATE_ALIASES);
    let amount_col = alias_index(headers, AMOUNT_ALIASES);
    let note_col = alias_index(headers, NOTE_ALIASES);
    let payee_col = alias_index(headers, PAYEE_ALIASES);
    let external_id_col = alias_index(headers, EXTERNAL_ID_ALIASES);
    let currency_col = alias_index(headers, CURRENCY_ALIASES);
    let type_col = alias_index(headers, TYPE_ALIASES);
    if date_col.is_none() || amount_col.is_none() {
        return Err(KokuError::InvalidInput(
            "CSV 缺少必需的日期/金额列；请检查表头是否包含「日期」与「金额」".to_owned(),
        ));
    }

    let mut rows = Vec::new();
    let mut issues = Vec::new();
    for (index, record) in reader.records().enumerate() {
        let line = index as u32 + 2;
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                issues.push(ParseIssue {
                    line,
                    message: format!("无法解析该行: {error}"),
                });
                continue;
            }
        };
        let get = |column: Option<usize>| column.and_then(|index| record.get(index)).unwrap_or("");
        let Some(date) = parse_date(get(date_col)) else {
            issues.push(ParseIssue {
                line,
                message: format!("无效日期: {}", get(date_col)),
            });
            continue;
        };
        let Some(mut amount) = parse_amount(get(amount_col)) else {
            issues.push(ParseIssue {
                line,
                message: format!("无效金额: {}", get(amount_col)),
            });
            continue;
        };
        // 可选「收支类型」列显式覆盖符号（如银行给出 支出/收入 而非带符号金额）。
        if let Some(type_index) = type_col {
            let raw = get(Some(type_index)).trim();
            if !raw.is_empty() {
                let lower = raw.to_lowercase();
                if lower.contains("income")
                    || lower.contains("收入")
                    || lower.contains("credit")
                    || lower.contains("收")
                {
                    amount = amount.abs();
                } else if lower.contains("expense")
                    || lower.contains("支出")
                    || lower.contains("debit")
                    || lower.contains("支")
                {
                    amount = -amount.abs();
                }
            }
        }
        if amount.is_zero() {
            issues.push(ParseIssue {
                line,
                message: "跳过金额为零的流水".to_owned(),
            });
            continue;
        }
        let note = non_empty(get(note_col)).unwrap_or("").to_owned();
        // 独立「商户」列作为商户识别原始文本；无独立列时回退备注（保持兼容）。
        let raw_description = non_empty(get(payee_col)).map(str::to_owned).or_else(|| {
            if note.is_empty() {
                None
            } else {
                Some(note.clone())
            }
        });
        rows.push(ImportRow {
            line,
            date,
            amount,
            note,
            currency: non_empty(get(currency_col)).map(str::to_owned),
            category_name: None,
            settled_amount: None,
            payee_name: None,
            raw_description,
            external_id: non_empty(get(external_id_col)).map(str::to_owned),
        });
    }
    Ok((rows, issues))
}

// ---------------------------------------------------------------------------
// QIF
// ---------------------------------------------------------------------------

/// 解析 QIF：`!Type:Bank/CCard/Cash` 段的 `D`（日期）、`T`（金额）、
/// `P`（收款方）、`M`（备注）字段，`^` 结束一条记录。
pub fn parse_qif(input: &str) -> Result<ParseOutcome> {
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    let mut active_type = String::new();
    let mut record_no: u32 = 0;
    // (日期, 金额, 收款方, 备注)
    let mut date: Option<NaiveDate> = None;
    let mut amount: Option<Decimal> = None;
    let mut payee = String::new();
    let mut memo = String::new();

    let mut flush = |date: &mut Option<NaiveDate>,
                     amount: &mut Option<Decimal>,
                     payee: &mut String,
                     memo: &mut String,
                     record_no: u32| {
        if let (Some(date), Some(amount)) = (date.take(), amount.take()) {
            if amount.is_zero() {
                issues.push(ParseIssue {
                    line: record_no,
                    message: "跳过金额为零的流水".to_owned(),
                });
            } else {
                // P（收款方）作为商户识别原始文本，M（备注）作为用户备注。
                let note = memo.trim().to_owned();
                let raw_description = if payee.trim().is_empty() {
                    None
                } else {
                    Some(payee.trim().to_owned())
                };
                rows.push(ImportRow {
                    line: record_no,
                    date,
                    amount,
                    note,
                    currency: None,
                    category_name: None,
                    settled_amount: None,
                    payee_name: None,
                    raw_description,
                    external_id: None,
                });
            }
        } else {
            issues.push(ParseIssue {
                line: record_no,
                message: "跳过不完整记录（缺日期或金额）".to_owned(),
            });
        }
        payee.clear();
        memo.clear();
    };

    for (index, raw) in input.lines().enumerate() {
        let line_no = index as u32 + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('!') {
            active_type = rest.trim().to_ascii_lowercase();
            continue;
        }
        if !matches!(
            active_type.as_str(),
            "type:bank" | "type:ccard" | "type:cash"
        ) {
            continue;
        }
        match line.chars().next() {
            Some('^') => {
                record_no += 1;
                flush(&mut date, &mut amount, &mut payee, &mut memo, record_no);
            }
            Some('D') => {
                date = parse_date(&line[1..]);
            }
            Some('T') | Some('U') => {
                amount = parse_amount(&line[1..]);
            }
            Some('P') => {
                payee = line[1..].trim().to_owned();
            }
            Some('M') => {
                memo = line[1..].trim().to_owned();
            }
            _ => {}
        }
        let _ = line_no;
    }
    if date.is_some() || amount.is_some() {
        record_no += 1;
        flush(&mut date, &mut amount, &mut payee, &mut memo, record_no);
    }
    Ok((rows, issues))
}

// ---------------------------------------------------------------------------
// OFX
// ---------------------------------------------------------------------------

/// 解析 OFX：扫描 `<STMTTRN>` 块，提取 DTPOSTED（日期）、TRNAMT（带符号金额）、
/// NAME/MEMO（备注）、TRNTYPE（DEBIT=支出 / CREDIT=收入，覆盖符号）。
/// 对 OFX 1.x（SGML）与 2.x（XML）都有效，因为交易块的内层标签一致。
pub fn parse_ofx(input: &str) -> Result<ParseOutcome> {
    let upper = input.to_ascii_uppercase();
    let mut rows = Vec::new();
    let mut issues = Vec::new();
    let mut block_no: u32 = 0;
    let mut cursor = 0_usize;

    while let Some(relative) = upper[cursor..].find("<STMTTRN>") {
        let block_start = cursor + relative + "<STMTTRN>".len();
        let block_end = match upper[block_start..].find("</STMTTRN>") {
            Some(relative) => block_start + relative,
            None => {
                issues.push(ParseIssue {
                    line: block_no + 1,
                    message: "STMTTRN 块缺少闭合标签，忽略剩余内容".to_owned(),
                });
                break;
            }
        };
        cursor = block_end + "</STMTTRN>".len();
        block_no += 1;
        let block = &upper[block_start..block_end];
        let original = &input[block_start..block_end];

        let Some(raw_date) = extract_tag(block, original, "DTPOSTED") else {
            issues.push(ParseIssue {
                line: block_no,
                message: "缺少 DTPOSTED 日期字段".to_owned(),
            });
            continue;
        };
        let Some(date) = parse_date(&raw_date) else {
            issues.push(ParseIssue {
                line: block_no,
                message: format!("无效日期: {raw_date}"),
            });
            continue;
        };
        let Some(mut amount) =
            extract_tag(block, original, "TRNAMT").and_then(|value| parse_amount(&value))
        else {
            issues.push(ParseIssue {
                line: block_no,
                message: "缺少或无效的 TRNAMT 金额字段".to_owned(),
            });
            continue;
        };
        // TRNTYPE 显式声明借贷方向时覆盖符号。
        if let Some(trn_type) = extract_tag(block, original, "TRNTYPE") {
            let upper_type = trn_type.to_ascii_uppercase();
            if upper_type.contains("CREDIT") {
                amount = amount.abs();
            } else if upper_type.contains("DEBIT") {
                amount = -amount.abs();
            }
        }
        if amount.is_zero() {
            issues.push(ParseIssue {
                line: block_no,
                message: "跳过金额为零的流水".to_owned(),
            });
            continue;
        }
        let name = extract_tag(block, original, "NAME").unwrap_or_default();
        let memo = extract_tag(block, original, "MEMO").unwrap_or_default();
        // FITID 是银行提供的唯一流水号，仅用于去重，不进 note/raw_description。
        let external_id = extract_tag(block, original, "FITID");
        // NAME 优先作为商户识别原始文本，MEMO 作为用户备注；
        // NAME 为空时回退 MEMO（保持简单规则）。
        let note = memo.trim().to_owned();
        let raw_description = if name.trim().is_empty() {
            if memo.trim().is_empty() {
                None
            } else {
                Some(memo.trim().to_owned())
            }
        } else {
            Some(name.trim().to_owned())
        };
        rows.push(ImportRow {
            line: block_no,
            date,
            amount,
            note,
            currency: None,
            category_name: None,
            settled_amount: None,
            payee_name: None,
            raw_description,
            external_id,
        });
    }
    Ok((rows, issues))
}

/// 在 OFX 块中提取某个标签的值（块内首个出现）：从 `<TAG>` 之后读到下一个 `<`。
/// `upper` 用于大小写不敏感定位，`original` 用于保留原始大小写文本。
/// 两个切片共享同一字节偏移，`to_ascii_uppercase` 不改变字节长度。
fn extract_tag(upper_block: &str, original_block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let relative = upper_block.find(&open)?;
    let value_start = relative + open.len();
    let value_end = upper_block[value_start..]
        .find('<')
        .map(|offset| value_start + offset)
        .unwrap_or(upper_block.len());
    let value = original_block[value_start..value_end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

// ---------------------------------------------------------------------------
// 通用解析工具
// ---------------------------------------------------------------------------

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 解析带符号金额：支持千分位、括号负数 `(123.45)`、尾随负号 `123.45-`、
/// 货币符号前缀、逗号/点两种小数分隔习惯。
pub fn parse_amount(raw: &str) -> Option<Decimal> {
    let mut text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let mut negative = false;
    if text.starts_with('(') && text.ends_with(')') {
        negative = true;
        text = &text[1..text.len() - 1];
    }
    if text.ends_with('-') {
        negative = true;
        text = &text[..text.len() - 1];
    }
    if text.starts_with('-') {
        negative = true;
        text = &text[1..];
    }
    if text.starts_with('+') {
        text = &text[1..];
    }
    // 去掉货币符号与空白，保留数字、点、逗号。
    let cleaned: String = text
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',' || ch.is_whitespace())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let digits: String = cleaned
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',')
        .collect();

    // 千分位/小数分隔符消歧：
    // - 同时出现点与逗号：最后一个分隔符是小数点；
    // - 只有逗号：逗号后恰好 2 位数字视为小数点（欧洲习惯），否则视为千分位；
    // - 只有点：点后恰好 3 位数字且前面不止一位视为千分位（US 1,234 风格）。
    let normalized = if digits.contains('.') && digits.contains(',') {
        let last_dot = digits.rfind('.');
        let last_comma = digits.rfind(',');
        if last_dot > last_comma {
            // 点在小数位：逗号是千分位，去掉逗号。
            digits.replace(',', "")
        } else {
            // 逗号在小数位（欧洲习惯）：去掉千分位点，逗号转点。
            digits.replace('.', "").replace(',', ".")
        }
    } else if digits.contains(',') && !digits.contains('.') {
        let parts: Vec<&str> = digits.split(',').collect();
        if parts.len() == 2 && parts[1].len() == 2 {
            digits.replace(',', ".")
        } else {
            digits.replace(',', "")
        }
    } else if digits.contains('.') && !digits.contains(',') {
        let parts: Vec<&str> = digits.split('.').collect();
        if parts.len() == 2 && parts[1].len() == 3 && parts[0].len() > 1 {
            digits.replace('.', "")
        } else {
            digits
        }
    } else {
        digits
    };

    let mut parsed = Decimal::from_str_exact(&normalized).ok()?;
    if negative {
        parsed = -parsed;
    }
    Some(parsed)
}

/// 解析自然日：支持 YYYY-MM-DD / YYYY/MM/DD / YYYY.MM.DD / YYYYMMDD /
/// MM/DD/YYYY / DD/MM/YYYY / MM/DD/YY（含中文年月日）。
/// 斜杠/中划线歧义时按「月在前」（美国习惯）解析；若第一个数 > 12 则按日在前。
pub fn parse_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 只取日期部分（可能带时间）。
    let date_part = raw.split([' ', 'T', 't']).next().unwrap_or(raw).trim();

    // 中文：2024年3月15日 / 2024年03月15日
    if let Some(year_end) = date_part.find('年') {
        let after_year = &date_part[year_end..];
        let rest = &after_year['年'.len_utf8()..];
        if let Some(month_end) = rest.find('月') {
            let year: i32 = date_part[..year_end].parse().ok()?;
            let month: u32 = rest[..month_end].parse().ok()?;
            let day_part = &rest[month_end + '月'.len_utf8()..];
            let day: u32 = day_part.trim_end_matches('日').parse().ok()?;
            return NaiveDate::from_ymd_opt(year, month, day);
        }
    }

    // 数字格式：统一分隔符为 '/' 或 '-'
    for sep in ['/', '-', '.'] {
        if date_part.contains(sep) {
            let parts: Vec<&str> = date_part.split(sep).collect();
            if parts.len() == 3 {
                let a: u32 = parts[0].parse().ok()?;
                let b: u32 = parts[1].parse().ok()?;
                let c: u32 = parts[2].parse().ok()?;
                // YYYY-MM-DD / YYYY/MM/DD：首段是四位年份。
                if a >= 1000 {
                    return NaiveDate::from_ymd_opt(a as i32, b, c);
                }
                let year = if c > 1000 {
                    c as i32
                } else if c < 50 {
                    2000 + c as i32
                } else {
                    1900 + c as i32
                };
                // MM/DD/YYYY 或 DD/MM/YYYY：首段 > 12 时按日在前解析。
                let (month, day) = if a > 12 { (b, a) } else { (a, b) };
                if (1..=12).contains(&month) && (1..=31).contains(&day) {
                    if let Some(date) = NaiveDate::from_ymd_opt(year, month, day) {
                        return Some(date);
                    }
                }
                return None;
            }
        }
    }
    // YYYYMMDD（或带时分秒的 OFX DTPOSTED：取前 8 位数字）
    if date_part.len() >= 8 && date_part.chars().take(8).all(|ch| ch.is_ascii_digit()) {
        let year: i32 = date_part[0..4].parse().ok()?;
        let month: u32 = date_part[4..6].parse().ok()?;
        let day: u32 = date_part[6..8].parse().ok()?;
        return NaiveDate::from_ymd_opt(year, month, day);
    }
    // MM/DD'YY（QIF 常见）
    if date_part.contains('\'') {
        let parts: Vec<&str> = date_part.split(['/', '\'']).collect();
        if parts.len() == 3 {
            let month: u32 = parts[0].parse().ok()?;
            let day: u32 = parts[1].parse().ok()?;
            let year_suffix: u32 = parts[2].parse().ok()?;
            let year = 2000 + year_suffix as i32;
            return NaiveDate::from_ymd_opt(year, month, day);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_amount_variants() {
        assert_eq!(
            parse_amount("123.45").unwrap(),
            Decimal::from_str_exact("123.45").unwrap()
        );
        assert_eq!(
            parse_amount("-123.45").unwrap(),
            Decimal::from_str_exact("-123.45").unwrap()
        );
        assert_eq!(
            parse_amount("(123.45)").unwrap(),
            Decimal::from_str_exact("-123.45").unwrap()
        );
        assert_eq!(
            parse_amount("123.45-").unwrap(),
            Decimal::from_str_exact("-123.45").unwrap()
        );
        assert_eq!(
            parse_amount("¥1,234.56").unwrap(),
            Decimal::from_str_exact("1234.56").unwrap()
        );
        assert_eq!(
            parse_amount("1.234,56").unwrap(),
            Decimal::from_str_exact("1234.56").unwrap()
        );
        assert_eq!(
            parse_amount("1234,56").unwrap(),
            Decimal::from_str_exact("1234.56").unwrap()
        );
        assert_eq!(
            parse_amount("1,234").unwrap(),
            Decimal::from_str_exact("1234").unwrap()
        );
        assert_eq!(
            parse_amount("+88").unwrap(),
            Decimal::from_str_exact("88").unwrap()
        );
        assert_eq!(parse_amount(""), None);
        assert_eq!(parse_amount("abc"), None);
    }

    #[test]
    fn parses_date_variants() {
        let expect = NaiveDate::from_ymd_opt(2024, 3, 15).unwrap();
        assert_eq!(parse_date("2024-03-15"), Some(expect));
        assert_eq!(parse_date("2024/03/15"), Some(expect));
        assert_eq!(parse_date("2024.3.15"), Some(expect));
        assert_eq!(parse_date("20240315"), Some(expect));
        assert_eq!(parse_date("2024年3月15日"), Some(expect));
        assert_eq!(parse_date("03/15/2024"), Some(expect));
        assert_eq!(parse_date("15/03/2024"), Some(expect));
        assert_eq!(parse_date("03/15/24"), Some(expect));
        assert_eq!(parse_date("3/15/24"), Some(expect));
        assert_eq!(parse_date("2024-03-15 18:30:00"), Some(expect));
        assert_eq!(parse_date("20240315120000.000[-5:EST]"), Some(expect));
        assert_eq!(parse_date("not a date"), None);
    }

    #[test]
    fn generic_csv_parses_chinese_bank_statement() -> Result<()> {
        let input = "\
交易日期,交易金额,摘要,币种
2024-03-01,100.00,早餐,CNY
2024-03-02,-45.50,地铁,CNY
2024-03-03,2000.00,工资,CNY
坏行,
";
        let (rows, issues) = parse_csv(input)?;
        assert_eq!(rows.len(), 3);
        assert_eq!(issues.len(), 1); // 坏行缺金额
        assert_eq!(rows[0].amount, Decimal::from(100_u32));
        assert_eq!(rows[0].note, "早餐");
        assert_eq!(rows[1].amount, Decimal::from_str_exact("-45.50").unwrap());
        assert_eq!(rows[2].date, NaiveDate::from_ymd_opt(2024, 3, 3).unwrap());
        Ok(())
    }

    #[test]
    fn generic_csv_type_column_overrides_sign() -> Result<()> {
        let input = "\
date,amount,类型
2024-03-01,88,收入
2024-03-02,66,支出
";
        let (rows, _) = parse_csv(input)?;
        assert_eq!(rows[0].amount, Decimal::from(88_u32));
        assert_eq!(rows[1].amount, Decimal::from(-66_i32));
        Ok(())
    }

    #[test]
    fn koku_export_csv_roundtrips() -> Result<()> {
        let input = "\
id,kind,account,target_account,category,amount,currency,settled_amount,occurred_at,note,voided_at
1,expense,零钱,,餐饮,30.00,CNY,30.00,2024-03-01T12:00:00Z,午餐,
2,income,零钱,,工资,5000.00,CNY,5000.00,2024-03-05T09:00:00Z,三月工资,
3,transfer,零钱,储蓄,,200.00,CNY,200.00,2024-03-06T10:00:00Z,转账,
";
        let (rows, issues) = parse_csv(input)?;
        assert_eq!(rows.len(), 2); // transfer 行被跳过
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("transfer"));
        assert_eq!(rows[0].category_name.as_deref(), Some("餐饮"));
        assert_eq!(rows[0].amount, Decimal::from(-30_i32));
        assert_eq!(rows[1].amount, Decimal::from(5000_u32));
        Ok(())
    }

    #[test]
    fn koku_export_csv_roundtrips_payee_and_raw_description() -> Result<()> {
        let input = "\
id,kind,account,target_account,category,payee,amount,currency,settled_amount,occurred_at,note,raw_description,voided_at
1,expense,零钱,,餐饮,饿了么,25.50,CNY,25.50,2024-03-01T12:00:00Z,午饭,支付宝-上海拉扎斯信息科技有限公司20260815001,
";
        let (rows, issues) = parse_csv(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payee_name.as_deref(), Some("饿了么"));
        assert_eq!(
            rows[0].raw_description.as_deref(),
            Some("支付宝-上海拉扎斯信息科技有限公司20260815001")
        );
        assert_eq!(rows[0].category_name.as_deref(), Some("餐饮"));
        Ok(())
    }

    #[test]
    fn legacy_koku_csv_without_payee_columns_still_parses() -> Result<()> {
        let input = "\
id,kind,account,target_account,category,amount,currency,settled_amount,occurred_at,note,voided_at
1,expense,零钱,,餐饮,30.00,CNY,30.00,2024-03-01T12:00:00Z,午餐,
";
        let (rows, issues) = parse_csv(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].payee_name.is_none());
        assert!(rows[0].raw_description.is_none());
        Ok(())
    }

    #[test]
    fn generic_csv_uses_payee_column_as_raw_description() -> Result<()> {
        let input = "\
日期,金额,交易对方,备注
2024-03-01,-25.50,麦当劳,午餐
2024-03-02,3000.00,雇主,工资
";
        let (rows, issues) = parse_csv(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        // 独立商户列 → raw_description；备注列 → note。
        assert_eq!(rows[0].raw_description.as_deref(), Some("麦当劳"));
        assert_eq!(rows[0].note, "午餐");
        assert_eq!(rows[0].amount, Decimal::from_str_exact("-25.50").unwrap());
        assert_eq!(rows[1].raw_description.as_deref(), Some("雇主"));
        assert_eq!(rows[1].note, "工资");
        Ok(())
    }

    #[test]
    fn generic_csv_without_payee_column_falls_back_to_note() -> Result<()> {
        let input = "\
日期,金额,备注
2024-03-01,-25.50,星巴克 - 早餐
";
        let (rows, issues) = parse_csv(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 1);
        // 无独立商户列：raw_description 回退备注，保持兼容。
        assert_eq!(rows[0].raw_description.as_deref(), Some("星巴克 - 早餐"));
        assert_eq!(rows[0].note, "星巴克 - 早餐");
        Ok(())
    }

    #[test]
    fn qif_parses_bank_records() -> Result<()> {
        let input = "\
!Type:Bank
D03/15/2024
T-100.00
P星巴克
M早餐
^
D03/16/2024
T2000.00
P工资
^
";
        let (rows, issues) = parse_qif(input)?;
        assert_eq!(rows.len(), 2);
        assert!(issues.is_empty());
        assert_eq!(rows[0].amount, Decimal::from(-100_i32));
        // P（收款方）→ raw_description；M（备注）→ note，不再拼接。
        assert_eq!(rows[0].raw_description.as_deref(), Some("星巴克"));
        assert_eq!(rows[0].note, "早餐");
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2024, 3, 15).unwrap());
        // 只有 P 没有 M：raw_description 保留，note 为空。
        assert_eq!(rows[1].raw_description.as_deref(), Some("工资"));
        assert_eq!(rows[1].note, "");
        assert_eq!(rows[1].amount, Decimal::from(2000_u32));
        Ok(())
    }

    #[test]
    fn ofx_parses_sgml_and_xml_blocks() -> Result<()> {
        let input = "\
OFXHEADER:100
<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>
<STMTTRN>
<TRNTYPE>DEBIT
<DTPOSTED>20240315120000.000[-5:EST]
<TRNAMT>-45.50
<NAME>Whole Foods</NAME>
<MEMO>Groceries</MEMO>
</STMTTRN>
<STMTTRN>
<TRNTYPE>CREDIT
<DTPOSTED>20240316
<TRNAMT>2500.00
<NAME>Employer</NAME>
<MEMO></MEMO>
</STMTTRN>
</BANKTRANLIST></STMTRS></STMTTRNRS></OFX>
";
        let (rows, issues) = parse_ofx(input)?;
        assert_eq!(rows.len(), 2);
        assert!(issues.is_empty());
        assert_eq!(rows[0].amount, Decimal::from_str_exact("-45.50").unwrap());
        // NAME → raw_description；MEMO → note，不再拼接。
        assert_eq!(rows[0].raw_description.as_deref(), Some("Whole Foods"));
        assert_eq!(rows[0].note, "Groceries");
        assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2024, 3, 15).unwrap());
        // 只有 NAME 没有 MEMO：raw_description 保留，note 为空。
        assert_eq!(rows[1].raw_description.as_deref(), Some("Employer"));
        assert_eq!(rows[1].note, "");
        assert_eq!(rows[1].amount, Decimal::from(2500_u32));
        Ok(())
    }

    #[test]
    fn sniff_detects_formats() {
        assert_eq!(
            sniff_format("!Type:Bank\nD01/01/2024\nT-1.00\n^"),
            ImportFormat::Qif
        );
        assert_eq!(
            sniff_format("<OFX><STMTTRN><TRNAMT>-1.00</TRNAMT></STMTTRN></OFX>"),
            ImportFormat::Ofx
        );
        assert_eq!(
            sniff_format("date,amount\n2024-01-01,1.00"),
            ImportFormat::Csv
        );
    }
}
