//! 批量导入流水：把 [`crate::importer`] 解析出的行写入账本。
//! 逐行处理：分类校验（行内分类名 > 默认分类）、跨币种结算金额折算
//! （行内明确给定 > 同币种原值 > 缓存汇率）、去重（外部唯一流水 ID 优先，
//! 账户/类型/时间/金额/币种/结算额/原始描述/备注字段指纹兜底）。单行失败
//! 不中断整批，错误汇总进 `issues` 返回前端。

use rusqlite::params;
use serde::Serialize;

use super::*;
use crate::auth::generate_session_token;
use crate::domain::{CategoryKind, CategorySuggestion, TransactionKind};
use crate::error::{KokuError, Result};
use crate::importer::{ImportFormat, ImportRow, ParseIssue};
use crate::service::BookkeepingService;

/// 导入报告只保留有限条失败明细与分类建议，计数仍完整，避免坏账单把响应撑爆。
const MAX_IMPORT_REPORT_ITEMS: usize = 500;

/// 一次导入的统计结果。
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    /// 本次导入批次；可通过对应 API 整批撤销（软撤销）。
    pub batch_id: String,
    pub format: String,
    pub account_id: i64,
    pub imported: usize,
    /// 与现有流水指纹重复而被跳过的条数。
    pub skipped_duplicates: usize,
    /// 解析或写入失败的行数（明细在 `issues`）。
    pub failed: usize,
    /// 被跳过的行（重复、零金额、缺字段、缺分类等）。
    pub issues: Vec<ParseIssue>,
    /// 成功导入且识别出商户（Payee）的条数。
    pub payees_recognized: usize,
    /// 成功导入且按历史统计高置信度自动应用分类的条数。
    pub categories_auto_applied: usize,
    /// 成功导入且产生中等置信度分类建议（未自动应用）的条数。
    pub category_suggestion_count: usize,
    /// 中等置信度分类建议明细（每条对应一笔已导入交易，等待人工确认）。
    pub category_suggestions: Vec<CategorySuggestion>,
    /// 成功导入但未能识别商户的条数。
    pub unrecognized: usize,
}

/// 单行导入结果（统计用）。
struct ImportRowOutcome {
    imported: bool,
    transaction_id: Option<i64>,
    payee_recognized: bool,
    category_auto_applied: bool,
    suggestion: Option<CategorySuggestion>,
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
        let mut payees_recognized = 0_usize;
        let mut categories_auto_applied = 0_usize;
        let mut category_suggestion_count = 0_usize;
        let mut category_suggestions = Vec::new();
        let mut unrecognized = 0_usize;
        let batch_id = generate_session_token()?;
        self.conn.execute(
            "INSERT INTO import_batches(id, created_at) VALUES (?1, ?2)",
            params![batch_id, timestamp(Utc::now())],
        )?;
        for row in rows {
            match self.import_row(
                account_id,
                &account.currency,
                &default_currency,
                default_category_id,
                &row,
            ) {
                Ok(outcome) => {
                    if outcome.imported {
                        let transaction_id = outcome
                            .transaction_id
                            .expect("imported rows have a transaction id");
                        self.conn.execute(
                            "UPDATE transactions SET import_batch_id = ?1 WHERE id = ?2",
                            params![batch_id, transaction_id],
                        )?;
                        imported += 1;
                        if outcome.payee_recognized {
                            payees_recognized += 1;
                        } else {
                            unrecognized += 1;
                        }
                        if outcome.category_auto_applied {
                            categories_auto_applied += 1;
                        }
                        if let Some(suggestion) = outcome.suggestion {
                            category_suggestion_count += 1;
                            if category_suggestions.len() < MAX_IMPORT_REPORT_ITEMS {
                                category_suggestions.push(suggestion);
                            }
                        }
                    } else {
                        skipped_duplicates += 1;
                    }
                }
                Err(error) => {
                    failed += 1;
                    if issues.len() < MAX_IMPORT_REPORT_ITEMS {
                        issues.push(ParseIssue {
                            line: row.line,
                            message: error.to_string(),
                        });
                    }
                }
            }
        }
        Ok(ImportResult {
            batch_id,
            format: format.as_str().to_owned(),
            account_id,
            imported,
            skipped_duplicates,
            failed,
            issues,
            payees_recognized,
            categories_auto_applied,
            category_suggestion_count,
            category_suggestions,
            unrecognized,
        })
    }

    /// 软撤销某次导入创建的全部流水，保留审计轨迹与外部流水号去重保护。
    pub fn undo_import_batch(&mut self, batch_id: &str) -> Result<usize> {
        let undone_at: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT undone_at FROM import_batches WHERE id = ?1",
                [batch_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(undone_at) = undone_at else {
            return Err(KokuError::NotFound {
                entity: "import batch",
                id: 0,
            });
        };
        if undone_at.is_some() {
            return Err(KokuError::InvalidInput(
                "import batch has already been undone".to_owned(),
            ));
        }
        let ids = {
            let mut statement = self.conn.prepare(
                "SELECT id FROM transactions WHERE import_batch_id = ?1 AND voided_at IS NULL ORDER BY id DESC",
            )?;
            let ids = statement
                .query_map([batch_id], |row| row.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };
        for id in &ids {
            self.void_transaction(*id)?;
        }
        self.conn.execute(
            "UPDATE import_batches SET undone_at = ?1 WHERE id = ?2",
            params![timestamp(Utc::now()), batch_id],
        )?;
        Ok(ids.len())
    }

    /// 校验并返回导入默认分类；未提供时报错（与既有行为一致）。
    fn default_category_for_import(
        &self,
        default_category_id: Option<i64>,
        expected_kind: CategoryKind,
    ) -> Result<i64> {
        match default_category_id {
            Some(id) => {
                let category = self.category(id)?;
                if category.kind != expected_kind {
                    return Err(KokuError::CategoryKindMismatch {
                        expected: expected_kind.as_str(),
                        actual: category.kind.as_str(),
                    });
                }
                Ok(id)
            }
            None => Err(KokuError::InvalidInput(
                "缺少分类：请在导入时选择默认分类，或使用带分类列的 Koku 导出 CSV".to_owned(),
            )),
        }
    }

    /// 写入单行：返回 `true` 已导入、`false` 判定为重复跳过（含统计信息）。
    #[allow(clippy::too_many_arguments)]
    fn import_row(
        &mut self,
        account_id: i64,
        account_currency: &str,
        default_currency: &str,
        default_category_id: Option<i64>,
        row: &ImportRow,
    ) -> Result<ImportRowOutcome> {
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
        // 原始描述：Koku 导出 CSV 明确给出时优先，否则用备注（通用银行流水）。
        let raw_description = row
            .raw_description
            .clone()
            .unwrap_or_else(|| row.note.clone());

        // 商户识别：行内 payee_name（Koku 备份恢复，直接关联、不学习）优先；
        // 否则按归一化描述查 alias；命中后预测分类。
        let mut payee_recognized = false;
        let mut category_auto_applied = false;
        let mut suggestion: Option<CategorySuggestion> = None;
        let (payee_id, payee_name) = if let Some(name) = row
            .payee_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // 恢复 Payee：仅创建/复用商户并关联，不写 alias、不产生学习统计。
            payee_recognized = true;
            let payee = self.get_or_create_payee(name)?;
            (Some(payee.id), Some(payee.name.clone()))
        } else if raw_description.trim().is_empty() {
            (None, None)
        } else {
            match self.resolve_payee_for_description(&raw_description)? {
                Some(payee) => {
                    payee_recognized = true;
                    (Some(payee.id), Some(payee.name.clone()))
                }
                None => (None, None),
            }
        };
        let prediction = match payee_id {
            Some(pid) => self.predict_category(pid)?,
            None => None,
        };

        // 分类解析：行内分类名 > 高置信度预测（自动应用）> 默认分类；
        // 中等置信度预测只记录建议，不自动应用。
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
        } else if let Some(prediction) = prediction {
            if prediction.auto_applied {
                category_auto_applied = true;
                prediction.category_id
            } else {
                // 中置信度：真实分类保持默认，同时返回建议供用户确认。
                // 默认分类与预测分类相同 → 建议无意义，不生成（也不算自动应用）。
                let default_id =
                    self.default_category_for_import(default_category_id, expected_kind)?;
                if default_id != prediction.category_id {
                    let default_name = self.category(default_id)?.name;
                    suggestion = Some(CategorySuggestion {
                        transaction_id: 0, // 交易创建后填充
                        payee_id: payee_id.unwrap_or_default(),
                        payee_name: payee_name.clone().unwrap_or_default(),
                        current_category_id: default_id,
                        current_category_name: default_name,
                        suggested_category_id: prediction.category_id,
                        suggested_category_name: prediction.category_name,
                        confidence: prediction.confidence,
                    });
                }
                default_id
            }
        } else {
            self.default_category_for_import(default_category_id, expected_kind)?
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

        // 两级去重：external_id 优先，字段指纹兜底。
        // - 有 external_id：同一账户下同 ID 即重复（不同 external_id 即使内容全同
        //   也视为两笔）；voided 的交易保留 external_id，仍算重复。
        // - 无 external_id：字段指纹（账户/类型/时间/金额/币种/结算额/原始描述/备注），
        //   已撤销交易不参与匹配。
        let duplicate: Option<i64> = match row
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(external_id) => self
                .conn
                .query_row(
                    "SELECT 1 FROM transactions
                     WHERE account_id = ?1 AND import_external_id = ?2
                     LIMIT 1",
                    params![account_id, external_id],
                    |row| row.get(0),
                )
                .optional()?,
            None => self
                .conn
                .query_row(
                    "SELECT 1 FROM transactions
                     WHERE account_id = ?1 AND kind = ?2 AND occurred_at = ?3
                       AND amount = ?4 AND currency = ?5 AND settled_amount = ?6
                       AND COALESCE(raw_description, '') = ?7 AND note = ?8
                       AND voided_at IS NULL
                     LIMIT 1",
                    params![
                        account_id,
                        kind.as_str(),
                        timestamp(occurred_at),
                        decimal_to_db(amount),
                        currency,
                        decimal_to_db(settled_amount),
                        raw_description,
                        row.note
                    ],
                    |row| row.get(0),
                )
                .optional()?,
        };
        if duplicate.is_some() {
            return Ok(ImportRowOutcome {
                imported: false,
                transaction_id: None,
                payee_recognized: false,
                category_auto_applied: false,
                suggestion: None,
            });
        }

        let transaction_id = match kind {
            TransactionKind::Expense => {
                let tx = self.record_expense_in_currency(
                    account_id,
                    category_id,
                    amount,
                    currency,
                    settled_amount,
                    occurred_at,
                    row.note.clone(),
                )?;
                // 保存原始描述与识别出的商户（学习数据只由人工操作产生，导入不学习）。
                self.set_import_metadata(
                    tx.id,
                    &raw_description,
                    payee_id,
                    row.external_id.as_deref(),
                )?;
                self.apply_rules_to_transaction(tx.id)?;
                if let Some(s) = suggestion.as_mut() {
                    s.transaction_id = tx.id;
                }
                tx.id
            }
            TransactionKind::Income => {
                let tx = self.record_income_in_currency(
                    account_id,
                    category_id,
                    amount,
                    currency,
                    settled_amount,
                    occurred_at,
                    row.note.clone(),
                )?;
                self.set_import_metadata(
                    tx.id,
                    &raw_description,
                    payee_id,
                    row.external_id.as_deref(),
                )?;
                self.apply_rules_to_transaction(tx.id)?;
                if let Some(s) = suggestion.as_mut() {
                    s.transaction_id = tx.id;
                }
                tx.id
            }
            _ => unreachable!("import only produces income/expense"),
        };
        Ok(ImportRowOutcome {
            imported: true,
            transaction_id: Some(transaction_id),
            payee_recognized,
            category_auto_applied,
            suggestion,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use crate::importer::{parse_ofx, parse_qif};
    use crate::service::payees::{AUTO_CATEGORY_CONFIDENCE, SUGGEST_CATEGORY_CONFIDENCE};
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
            payee_name: None,
            raw_description: None,
            external_id: None,
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

    /// 通过真实交易确认一笔 (Payee, Category) 学习样本（预置导入学习数据用）。
    fn learn(service: &mut BookkeepingService, payee_id: i64, category_id: i64) -> Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let account = service.create_account(
            format!("预置学习-{n}"),
            AccountType::Cash,
            "CNY",
            Decimal::from(100000_u32),
        )?;
        let tx = service.record_expense(
            account.id,
            category_id,
            Decimal::from(10_u32),
            chrono::Utc::now(),
            "预置样本",
        )?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee_id, tx.id],
        )?;
        service.confirm_transaction_learning(tx.id)
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
    fn undoing_an_import_batch_voids_only_its_transactions() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let existing = service.record_expense(
            account_id,
            food_id,
            Decimal::from(20_u32),
            chrono::Utc::now(),
            "导入前已有流水",
        )?;
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![row(1, date(2026, 3, 1), "-100.00", "待撤销导入")],
        )?;

        assert_eq!(result.imported, 1);
        assert_eq!(service.undo_import_batch(&result.batch_id)?, 1);
        let transactions = service.transactions(100, 0)?;
        assert_eq!(
            transactions
                .iter()
                .filter(|item| item.voided_at.is_none())
                .count(),
            1
        );
        assert_eq!(
            transactions
                .iter()
                .filter(|item| item.voided_at.is_some())
                .count(),
            1
        );
        assert!(service.transaction(existing.id)?.voided_at.is_none());
        assert!(service.undo_import_batch(&result.batch_id).is_err());
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

    #[test]
    fn import_recognizes_payee_and_auto_applies_high_confidence_category() -> Result<()> {
        let (mut service, account_id, _) = seeded_service()?;
        let shopping = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "购物")
            .unwrap();
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        // 预置学习数据：描述 → 饿了么；饿了么 → 餐饮（5 次，高置信度）。
        let eleme = service.get_or_create_payee("饿了么")?;
        service.learn_alias("支付宝-上海拉扎斯信息科技有限公司", eleme.id)?;
        for _ in 0..5 {
            learn(&mut service, eleme.id, food.id)?;
        }
        let mut imported_row = row(
            1,
            date(2026, 3, 1),
            "-25.50",
            "支付宝-上海拉扎斯信息科技有限公司",
        );
        imported_row.note = "支付宝-上海拉扎斯信息科技有限公司20260815001".to_owned();
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            // 默认分类故意设为购物：高置信度预测应覆盖默认分类。
            Some(shopping.id),
            None,
            vec![imported_row],
        )?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.payees_recognized, 1);
        assert_eq!(result.categories_auto_applied, 1);
        assert_eq!(result.category_suggestion_count, 0);
        assert!(result.category_suggestions.is_empty());
        assert_eq!(result.unrecognized, 0);
        // 预置学习也创建了交易，按导入备注定位导入的那条。
        let transaction = service
            .transactions(100, 0)?
            .into_iter()
            .find(|item| item.note == "支付宝-上海拉扎斯信息科技有限公司20260815001")
            .unwrap();
        assert_eq!(transaction.payee_id, Some(eleme.id));
        assert_eq!(transaction.category_id, Some(food.id));
        assert_eq!(
            transaction.raw_description.as_deref(),
            Some("支付宝-上海拉扎斯信息科技有限公司20260815001")
        );
        Ok(())
    }

    #[test]
    fn import_records_suggestion_without_auto_apply() -> Result<()> {
        let (mut service, account_id, _) = seeded_service()?;
        let shopping = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "购物")
            .unwrap();
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        // 京东分类分布较散（7 餐饮 / 3 购物）→ 只出建议，不自动应用。
        let jd = service.get_or_create_payee("京东")?;
        service.learn_alias("京东商城", jd.id)?;
        for _ in 0..7 {
            learn(&mut service, jd.id, food.id)?;
        }
        for _ in 0..3 {
            learn(&mut service, jd.id, shopping.id)?;
        }
        let mut imported_row = row(1, date(2026, 3, 2), "-99.00", "京东商城");
        imported_row.note = "支付宝-京东商城".to_owned();
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(shopping.id),
            None,
            vec![imported_row],
        )?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.payees_recognized, 1);
        assert_eq!(result.categories_auto_applied, 0);
        assert_eq!(result.category_suggestion_count, 1);
        assert_eq!(result.category_suggestions.len(), 1);
        let suggestion = &result.category_suggestions[0];
        assert_eq!(suggestion.payee_name, "京东");
        assert_eq!(suggestion.current_category_id, shopping.id);
        assert_eq!(suggestion.suggested_category_id, food.id);
        assert!(suggestion.confidence >= SUGGEST_CATEGORY_CONFIDENCE);
        assert!(suggestion.confidence < AUTO_CATEGORY_CONFIDENCE);
        assert_eq!(result.unrecognized, 0);
        let transaction = service
            .transactions(100, 0)?
            .into_iter()
            .find(|item| item.note == "支付宝-京东商城")
            .unwrap();
        assert_eq!(transaction.payee_id, Some(jd.id));
        // 建议未自动应用：分类保持默认「购物」。
        assert_eq!(transaction.category_id, Some(shopping.id));
        Ok(())
    }

    /// Koku 自身备份 CSV round-trip：恢复 Payee 与原始描述，
    /// 但导入只恢复数据，不写 alias、不产生学习统计。
    #[test]
    fn importing_koku_csv_restores_payee_without_learning() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let eleme = service.get_or_create_payee("饿了么")?;
        let mut imported_row = row(1, date(2026, 3, 1), "-25.50", "午饭");
        imported_row.category_name = Some("餐饮".to_owned());
        imported_row.payee_name = Some("饿了么".to_owned());
        imported_row.raw_description =
            Some("支付宝-上海拉扎斯信息科技有限公司20260815001".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![imported_row],
        )?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.payees_recognized, 1);
        assert_eq!(result.unrecognized, 0);
        let transaction = service.transactions(100, 0)?[0].clone();
        assert_eq!(transaction.payee_id, Some(eleme.id));
        assert_eq!(
            transaction.raw_description.as_deref(),
            Some("支付宝-上海拉扎斯信息科技有限公司20260815001")
        );
        // 导入只恢复数据：不写 alias、不产生学习统计。
        let aliases: i64 =
            service
                .conn
                .query_row("SELECT COUNT(*) FROM merchant_aliases", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(aliases, 0);
        let stats: i64 =
            service
                .conn
                .query_row("SELECT COUNT(*) FROM payee_category_stats", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(stats, 0);
        Ok(())
    }

    /// Koku 备份 CSV round-trip：payee_name 与 raw_description 缺省时仍正常导入。
    #[test]
    fn legacy_koku_csv_without_payee_columns_still_imports() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut imported_row = row(1, date(2026, 3, 1), "-25.50", "午饭");
        imported_row.category_name = Some("餐饮".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![imported_row],
        )?;
        assert_eq!(result.imported, 1);
        let transaction = service.transactions(100, 0)?[0].clone();
        assert!(transaction.payee_id.is_none());
        assert_eq!(transaction.raw_description.as_deref(), Some("午饭"));
        Ok(())
    }

    // ------------------------------------------------------------------
    // 去重指纹包含原始商户描述：同商户才是重复，不同商户不被误判
    // ------------------------------------------------------------------

    #[test]
    fn same_day_same_amount_different_payees_both_import() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut mcd = row(1, date(2026, 3, 1), "-30.00", "");
        mcd.raw_description = Some("麦当劳".to_owned());
        let mut sbux = row(2, date(2026, 3, 1), "-30.00", "");
        sbux.raw_description = Some("星巴克".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![mcd, sbux],
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        Ok(())
    }

    #[test]
    fn qif_payees_without_memo_are_not_deduplicated() -> Result<()> {
        let input = "\
!Type:Bank
D03/15/2024
T-30.00
P麦当劳
^
D03/15/2024
T-30.00
P星巴克
^
";
        let (rows, issues) = parse_qif(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        let (mut service, account_id, food_id) = seeded_service()?;
        let result = service.import_transactions(
            ImportFormat::Qif,
            account_id,
            Some(food_id),
            None,
            rows,
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        Ok(())
    }

    #[test]
    fn identical_rows_are_still_deduplicated() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.raw_description = Some("麦当劳".to_owned());
        let mut b = row(2, date(2026, 3, 1), "-30.00", "午餐");
        b.raw_description = Some("麦当劳".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a, b],
        )?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped_duplicates, 1);
        Ok(())
    }

    #[test]
    fn same_payee_amount_day_but_different_note_imports_both() -> Result<()> {
        // 保守原则：note 不同 → 视为不同交易，不误删。
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.raw_description = Some("麦当劳".to_owned());
        let mut b = row(2, date(2026, 3, 1), "-30.00", "晚餐");
        b.raw_description = Some("麦当劳".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a, b],
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        Ok(())
    }

    // ------------------------------------------------------------------
    // 无意义建议抑制：默认分类 == 预测分类时不生成建议
    // ------------------------------------------------------------------

    #[test]
    fn suggestion_matching_default_category_is_suppressed() -> Result<()> {
        let (mut service, account_id, _) = seeded_service()?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        let shopping = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "购物")
            .unwrap();
        // 京东：7 餐饮 / 3 购物 → 80% 属于建议区间。
        let jd = service.get_or_create_payee("京东")?;
        service.learn_alias("京东商城", jd.id)?;
        for _ in 0..7 {
            learn(&mut service, jd.id, food.id)?;
        }
        for _ in 0..3 {
            learn(&mut service, jd.id, shopping.id)?;
        }
        let mut imported_row = row(1, date(2026, 3, 2), "-99.00", "京东商城");
        imported_row.note = "支付宝-京东商城".to_owned();
        // 默认分类 = 餐饮（= 预测分类）→ 建议被抑制，普通成功导入。
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food.id),
            None,
            vec![imported_row],
        )?;
        assert_eq!(result.imported, 1);
        assert_eq!(result.category_suggestion_count, 0);
        assert!(result.category_suggestions.is_empty());
        assert_eq!(result.categories_auto_applied, 0);
        Ok(())
    }

    #[test]
    fn import_without_learning_data_counts_unrecognized() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let rows = vec![
            row(1, date(2026, 3, 1), "-10.00", "全新商户描述"),
            row(2, date(2026, 3, 2), "-20.00", "另一个新商户"),
        ];
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            rows,
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.payees_recognized, 0);
        assert_eq!(result.unrecognized, 2);
        assert_eq!(result.categories_auto_applied, 0);
        assert_eq!(result.category_suggestion_count, 0);
        assert!(result.category_suggestions.is_empty());
        Ok(())
    }

    // ------------------------------------------------------------------
    // 外部唯一流水 ID（external_id）去重
    // ------------------------------------------------------------------

    #[test]
    fn same_content_different_external_ids_both_import() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.raw_description = Some("麦当劳".to_owned());
        a.external_id = Some("EXT-1".to_owned());
        let mut b = row(2, date(2026, 3, 1), "-30.00", "午餐");
        b.raw_description = Some("麦当劳".to_owned());
        b.external_id = Some("EXT-2".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a, b],
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        Ok(())
    }

    #[test]
    fn same_external_id_is_deduplicated_on_reimport() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.raw_description = Some("麦当劳".to_owned());
        a.external_id = Some("EXT-1".to_owned());
        let first = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a],
        )?;
        assert_eq!(first.imported, 1);
        // 同一 external_id 再导入（内容不同也不重要）→ 重复。
        let mut b = row(2, date(2026, 3, 2), "-99.00", "完全不同");
        b.external_id = Some("EXT-1".to_owned());
        let second = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![b],
        )?;
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 1);
        Ok(())
    }

    #[test]
    fn same_external_id_in_different_accounts_both_import() -> Result<()> {
        let (mut service, _, food_id) = seeded_service()?;
        let account_a =
            service.create_account("账户A", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let account_b =
            service.create_account("账户B", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.external_id = Some("EXT-1".to_owned());
        let mut b = row(2, date(2026, 3, 1), "-30.00", "午餐");
        b.external_id = Some("EXT-1".to_owned());
        let result_a = service.import_transactions(
            ImportFormat::Csv,
            account_a.id,
            Some(food_id),
            None,
            vec![a],
        )?;
        let result_b = service.import_transactions(
            ImportFormat::Csv,
            account_b.id,
            Some(food_id),
            None,
            vec![b],
        )?;
        assert_eq!(result_a.imported, 1);
        assert_eq!(result_b.imported, 1);
        Ok(())
    }

    #[test]
    fn without_external_id_fallback_fingerprint_still_works() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.raw_description = Some("麦当劳".to_owned());
        let mut dup = row(2, date(2026, 3, 1), "-30.00", "午餐");
        dup.raw_description = Some("麦当劳".to_owned());
        let mut other = row(3, date(2026, 3, 1), "-30.00", "早餐");
        other.raw_description = Some("星巴克".to_owned());
        let result = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a, dup, other],
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 1);
        Ok(())
    }

    #[test]
    fn voided_transaction_keeps_external_id_and_blocks_reimport() -> Result<()> {
        let (mut service, account_id, food_id) = seeded_service()?;
        let mut a = row(1, date(2026, 3, 1), "-30.00", "午餐");
        a.external_id = Some("EXT-1".to_owned());
        let first = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![a],
        )?;
        assert_eq!(first.imported, 1);
        let tx = service.transactions(100, 0)?[0].clone();
        // void 保留 external_id → 再导入同一 external_id 仍算重复。
        service.void_transaction(tx.id)?;
        let mut b = row(2, date(2026, 3, 1), "-30.00", "午餐");
        b.external_id = Some("EXT-1".to_owned());
        let second = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![b.clone()],
        )?;
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 1);
        // permanent delete 后 external_id 随交易删除 → 允许重新导入。
        service.delete_transaction(tx.id)?;
        let third = service.import_transactions(
            ImportFormat::Csv,
            account_id,
            Some(food_id),
            None,
            vec![b],
        )?;
        assert_eq!(third.imported, 1);
        Ok(())
    }

    #[test]
    fn ofx_fitid_drives_deduplication() -> Result<()> {
        let input = "\
<OFX><STMTTRN>
<FITID>ID-1
<DTPOSTED>20240315120000.000[-5:EST]
<TRNAMT>-30.00
<NAME>麦当劳</NAME>
</STMTTRN>
<STMTTRN>
<FITID>ID-2
<DTPOSTED>20240315120000.000[-5:EST]
<TRNAMT>-30.00
<NAME>麦当劳</NAME>
</STMTTRN>
</OFX>
";
        let (rows, issues) = parse_ofx(input)?;
        assert!(issues.is_empty());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].external_id.as_deref(), Some("ID-1"));
        assert_eq!(rows[1].external_id.as_deref(), Some("ID-2"));
        let (mut service, account_id, food_id) = seeded_service()?;
        let result = service.import_transactions(
            ImportFormat::Ofx,
            account_id,
            Some(food_id),
            None,
            rows,
        )?;
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        Ok(())
    }
}
