//! 批量导入流水：把 [`crate::importer`] 解析出的行写入账本。
//! 逐行处理：分类校验（行内分类名 > 默认分类）、跨币种结算金额折算
//! （行内明确给定 > 同币种原值 > 缓存汇率）、按 (账户/类型/时间/金额/备注)
//! 指纹去重。单行失败不中断整批，错误汇总进 `issues` 返回前端。

use rusqlite::params;
use serde::Serialize;

use super::*;
use crate::domain::{CategoryKind, TransactionKind};
use crate::error::{KokuError, Result};
use crate::importer::{ImportFormat, ImportRow, ParseIssue};
use crate::service::BookkeepingService;

/// 一次导入的统计结果。
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub format: String,
    pub account_id: i64,
    pub imported: usize,
    /// 与现有流水指纹重复而被跳过的条数。
    pub skipped_duplicates: usize,
    /// 解析或写入失败的行数（明细在 `issues`）。
    pub failed: usize,
    /// 被跳过的行（重复、零金额、缺字段、缺分类等）。
    pub issues: Vec<ParseIssue>,
}

impl BookkeepingService {
    pub fn import_transactions(
        &mut self,
        format: ImportFormat,
        account_id: i64,
        default_category_id: Option<i64>,
        currency: Option<String>,
        rows: Vec<ImportRow>,
    ) -> Result<ImportResult> {
        let account = self.account(account_id)?;
        let default_currency = match currency {
            Some(value) => normalize_currency(value)?,
            None => account.currency.clone(),
        };
        let mut imported = 0_usize;
        let mut skipped_duplicates = 0_usize;
        let mut failed = 0_usize;
        let mut issues = Vec::new();
        for row in rows {
            match self.import_row(
                account_id,
                &account.currency,
                &default_currency,
                default_category_id,
                &row,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped_duplicates += 1,
                Err(error) => {
                    failed += 1;
                    issues.push(ParseIssue {
                        line: row.line,
                        message: error.to_string(),
                    });
                }
            }
        }
        Ok(ImportResult {
            format: format.as_str().to_owned(),
            account_id,
            imported,
            skipped_duplicates,
            failed,
            issues,
        })
    }

    /// 写入单行：返回 `true` 已导入、`false` 判定为重复跳过。
    #[allow(clippy::too_many_arguments)]
    fn import_row(
        &mut self,
        account_id: i64,
        account_currency: &str,
        default_currency: &str,
        default_category_id: Option<i64>,
        row: &ImportRow,
    ) -> Result<bool> {
        if row.amount.is_zero() {
            return Err(KokuError::InvalidInput("金额为零的流水无法导入".to_owned()));
        }
        let kind = if row.amount.is_sign_negative() {
            TransactionKind::Expense
        } else {
            TransactionKind::Income
        };
        let expected_kind = match kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            _ => unreachable!("import only produces income/expense"),
        };
        let amount = row.amount.abs();
        let currency = match &row.currency {
            Some(value) => normalize_currency(value.clone())?,
            None => default_currency.to_owned(),
        };

        // 分类解析：行内分类名 > 默认分类；都不满足则整行失败。
        let category_id = if let Some(name) = &row.category_name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(KokuError::InvalidInput("行内分类名为空".to_owned()));
            }
            self.conn
                .query_row(
                    "SELECT id FROM categories
                     WHERE name = ?1 AND kind = ?2 AND archived_at IS NULL",
                    params![trimmed, expected_kind.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    KokuError::InvalidInput(format!(
                        "账本中不存在{}分类: {trimmed}",
                        expected_kind.as_str()
                    ))
                })?
        } else if let Some(id) = default_category_id {
            let category = self.category(id)?;
            if category.kind != expected_kind {
                return Err(KokuError::CategoryKindMismatch {
                    expected: expected_kind.as_str(),
                    actual: category.kind.as_str(),
                });
            }
            id
        } else {
            return Err(KokuError::InvalidInput(
                "缺少分类：请在导入时选择默认分类，或使用带分类列的 Koku 导出 CSV".to_owned(),
            ));
        };

        let occurred_at = row
            .date
            .and_hms_opt(12, 0, 0)
            .ok_or_else(|| KokuError::InvalidInput("无效日期".to_owned()))?
            .and_utc();

        // 结算金额：行内明确给定 > 同币种取原值 > 用缓存汇率折算。
        let settled_amount = if let Some(value) = row.settled_amount {
            value
        } else if currency == account_currency {
            amount
        } else {
            let rate = self
                .conversion_rate(&currency, account_currency, row.date)?
                .ok_or_else(|| {
                    KokuError::InvalidInput(format!(
                        "缺少汇率 {currency}->{account_currency}，无法折算结算金额"
                    ))
                })?;
            (amount * rate).round_dp(2)
        };

        // 去重指纹：(账户, 类型, 时间, 结算金额, 备注)。
        let duplicate: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM transactions
                 WHERE account_id = ?1 AND kind = ?2 AND occurred_at = ?3
                   AND settled_amount = ?4 AND note = ?5 AND voided_at IS NULL
                 LIMIT 1",
                params![
                    account_id,
                    kind.as_str(),
                    timestamp(occurred_at),
                    decimal_to_db(settled_amount),
                    row.note
                ],
                |row| row.get(0),
            )
            .optional()?;
        if duplicate.is_some() {
            return Ok(false);
        }

        match kind {
            TransactionKind::Expense => {
                self.record_expense_in_currency(
                    account_id,
                    category_id,
                    amount,
                    currency,
                    settled_amount,
                    occurred_at,
                    row.note.clone(),
                )?;
            }
            TransactionKind::Income => {
                self.record_income_in_currency(
                    account_id,
                    category_id,
                    amount,
                    currency,
                    settled_amount,
                    occurred_at,
                    row.note.clone(),
                )?;
            }
            _ => unreachable!("import only produces income/expense"),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use chrono::NaiveDate;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    fn row(line: u32, date: NaiveDate, amount: &str, note: &str) -> ImportRow {
        ImportRow {
            line,
            date,
            amount: Decimal::from_str_exact(amount).unwrap(),
            note: note.to_owned(),
            currency: None,
            category_name: None,
            settled_amount: None,
        }
    }

    fn seeded_service() -> Result<(BookkeepingService, i64, i64)> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        Ok((service, account.id, food.id))
    }

    #[test]
    fn imports_rows_and_deduplicates_by_fingerprint() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut income_row = row(3, date(2026, 3, 3), "2000.00", "工资");
        income_row.category_name = Some("工资".to_owned());
        let rows = vec![
            row(1, date(2026, 3, 1), "-100.00", "早餐"),
            row(2, date(2026, 3, 2), "-45.50", "地铁"),
            income_row,
            // 与第 1 行完全重复 → 跳过
            row(4, date(2026, 3, 1), "-100.00", "早餐"),
            // 金额为零 → 失败
            row(5, date(2026, 3, 4), "0", "零金额"),
        ];
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            rows,
        )?;
        assert_eq!(result.imported, 3);
        assert_eq!(result.skipped_duplicates, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].message.contains("为零"));

        // 收支方向正确：餐饮支出从余额扣除，工资收入增加余额。
        let transactions = service.transactions(100, 0)?;
        assert_eq!(transactions.len(), 3);
        let expenses: Vec<Decimal> = transactions
            .iter()
            .filter(|item| item.kind == TransactionKind::Expense)
            .map(|item| item.amount)
            .collect();
        assert_eq!(expenses.len(), 2);
        assert!(expenses.contains(&Decimal::from_str_exact("100.00").unwrap()));
        assert!(expenses.contains(&Decimal::from_str_exact("45.50").unwrap()));
        let income = transactions
            .iter()
            .find(|item| item.kind == TransactionKind::Income)
            .unwrap();
        assert_eq!(income.amount, Decimal::from(2000_u32));
        let account = service.account(account_id)?;
        // 1000 - 100 - 45.50 + 2000
        assert_eq!(account.balance, Decimal::from_str_exact("2854.50").unwrap());
        Ok(())
    }

    #[test]
    fn requires_category_for_generic_rows() -> Result<()> {
        let (mut service, account_id, _) = seeded_service()?;
        let rows = vec![row(1, date(2026, 3, 1), "10.00", "无分类")];
        let result =
            service.import_transactions(ImportFormat::Csv, account_id, None, None, rows)?;
        assert_eq!(result.imported, 0);
        assert_eq!(result.failed, 1);
        assert!(result.issues[0].message.contains("分类"));
        Ok(())
    }

    #[test]
    fn category_name_from_export_overrides_default() -> Result<()> {
        let (mut service, account_id, _) = seeded_service()?;
        let salary = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "工资")
            .unwrap();
        let mut income_row = row(1, date(2026, 3, 1), "5000.00", "三月工资");
        income_row.category_name = Some("工资".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(salary.id),
            None,
            vec![income_row],
        )?;
        assert_eq!(result.imported, 1);
        let transaction = service.transactions(100, 0)?[0].clone();
        assert_eq!(transaction.category_id, Some(salary.id));
        Ok(())
    }

    #[test]
    fn cross_currency_rows_convert_via_cached_rate() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut usd_row = row(1, date(2026, 3, 1), "-10.00", "美元消费");
        usd_row.currency = Some("USD".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![usd_row],
        )?;
        // 无汇率缓存 → 整行失败
        assert_eq!(result.imported, 0);
        assert_eq!(result.failed, 1);
        assert!(result.issues[0].message.contains("汇率"));

        // 补上汇率后再次导入成功。
        service.store_rate(&crate::domain::RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str_exact("7.20").unwrap(),
            date: "2026-03-01".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;
        let mut usd_row = row(2, date(2026, 3, 1), "-10.00", "美元消费");
        usd_row.currency = Some("USD".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![usd_row],
        )?;
        assert_eq!(result.imported, 1);
        let transaction = service.transactions(100, 0)?[0].clone();
        assert_eq!(transaction.currency, "USD");
        assert_eq!(
            transaction.settled_amount,
            Decimal::from_str_exact("72.00").unwrap()
        );
        Ok(())
    }
}
