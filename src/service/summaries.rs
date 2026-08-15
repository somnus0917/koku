//! 月度统计：SQL 聚合的收入/支出/现金流汇总，所有币种按汇率折算到显示币种。

use std::collections::BTreeMap;

use chrono::Utc;
use rusqlite::params;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

use super::*;
use crate::domain::{CashFlowSummary, CategoryExpense, MonthlySummary, TransactionKind};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    pub fn monthly_summary(&self, year: i32, month: u32, currency: &str) -> Result<MonthlySummary> {
        let cash_flow = self.cash_flow_summary(year, month, currency)?;
        Ok(MonthlySummary {
            year,
            month,
            currency: cash_flow.currency,
            total_income: cash_flow.total_income,
            total_expense: cash_flow.total_expense,
            net: cash_flow.retained,
            expenses_by_category: cash_flow
                .expense_destinations
                .into_iter()
                .map(|item| CategoryExpense {
                    category_id: item.category_id,
                    category_name: item.category_name,
                    amount: item.amount,
                    percentage: item.percentage,
                })
                .collect(),
        })
    }

    /// 现金流汇总：`currency` 为显示币种，该月所有币种的收支统一折算到显示币种。
    /// 折算需要汇率缓存；调用方应先确保所需汇率可用（API 层会拉取缺失汇率），
    /// 缺汇率的币种会报错而不是被静默漏算。
    pub fn cash_flow_summary(
        &self,
        year: i32,
        month: u32,
        currency: &str,
    ) -> Result<CashFlowSummary> {
        let (start, end) = month_bounds(year, month)?;
        let currency = normalize_currency(currency.to_owned())?;
        let today = Utc::now().date_naive();
        let mut statement = self.conn.prepare(
            r#"
            SELECT t.kind, t.category_id, c.name, t.currency,
                   SUM(CAST(t.amount AS REAL)) - SUM(CAST(COALESCE(t.reimbursed_amount, '0') AS REAL))
            FROM transactions t
            JOIN categories c ON c.id = t.category_id
            WHERE t.voided_at IS NULL
              AND t.kind IN ('expense', 'income')
              AND t.occurred_at >= ?1 AND t.occurred_at < ?2
            GROUP BY t.kind, t.category_id, c.name, t.currency
            ORDER BY t.kind, t.category_id, t.currency
            "#,
        )?;
        let rows = statement.query_map(params![timestamp(start), timestamp(end)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;

        let mut total_income = Decimal::ZERO;
        let mut total_expense = Decimal::ZERO;
        let mut income_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        let mut expense_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        for row in rows {
            let (kind, category_id, category_name, tx_currency, sum) = row?;
            // SQLite 的 SUM 返回浮点，转回精确 Decimal 并取整到 4 位小数以消除浮点噪声；
            // 对货币金额（≤2 位小数）实测与精确 Decimal 求和完全一致。
            let net = Decimal::from_f64(sum)
                .ok_or_else(|| {
                    KokuError::InvalidInput("invalid monetary aggregate from database".to_owned())
                })?
                .round_dp(4);
            // 该币种的净额按汇率折算到显示币种。
            let amount = self.convert_amount(net, &tx_currency, &currency, today)?;
            match TransactionKind::from_db(&kind)? {
                TransactionKind::Income => {
                    total_income += amount;
                    *income_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Expense => {
                    total_expense += amount;
                    *expense_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Transfer | TransactionKind::Loan | TransactionKind::Adjustment => {
                }
            }
        }

        let income_sources = cash_flow_items(income_totals, total_income);
        let expense_destinations = cash_flow_items(expense_totals, total_expense);
        Ok(CashFlowSummary {
            year,
            month,
            currency,
            total_income,
            total_expense,
            retained: total_income - total_expense,
            flow_total: total_income.max(total_expense),
            income_sources,
            expense_destinations,
        })
    }

    /// 某月收支流水涉及的所有币种（供调用方确保折算汇率可用）。
    pub fn transaction_currencies(&self, year: i32, month: u32) -> Result<Vec<String>> {
        let (start, end) = month_bounds(year, month)?;
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT currency FROM transactions
             WHERE voided_at IS NULL AND kind IN ('expense', 'income')
               AND occurred_at >= ?1 AND occurred_at < ?2",
        )?;
        let rows = statement.query_map(params![timestamp(start), timestamp(end)], |row| {
            row.get::<_, String>(0)
        })?;
        let mut currencies = Vec::new();
        for row in rows {
            currencies.push(row?);
        }
        Ok(currencies)
    }
}
