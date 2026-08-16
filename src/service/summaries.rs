//! 月度统计：SQL 聚合的收入/支出/现金流汇总，所有币种按汇率折算到显示币种。

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};
use rusqlite::params;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

use super::*;
use crate::domain::{
    CashFlowSummary, CategoryExpense, MonthlySummary, MonthlyTrendPoint, TransactionKind,
};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    pub fn monthly_summary(&self, year: i32, month: u32, currency: &str) -> Result<MonthlySummary> {
        let cash_flow = self.cash_flow_summary(year, month, currency)?;
        let budget_limits = self.budget_limits(year, month)?;
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
                    budget_limit: budget_limits.get(&item.category_id).copied(),
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
                TransactionKind::Transfer
                | TransactionKind::Loan
                | TransactionKind::Adjustment
                | TransactionKind::Trade => {}
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

    /// 最近 `months` 个自然月（含当前月）的收支趋势：单条 SQL 按月/币种聚合，
    /// 再逐币种折算到显示币种。返回按时间升序的完整月序列，无流水的月份补零。
    pub fn monthly_trend(&self, months: u32, currency: &str) -> Result<Vec<MonthlyTrendPoint>> {
        let (start_index, start, end) = trend_bounds(months)?;
        let currency = normalize_currency(currency.to_owned())?;
        let today = Utc::now().date_naive();
        let mut statement = self.conn.prepare(
            r#"
            SELECT CAST(strftime('%Y', occurred_at) AS INTEGER),
                   CAST(strftime('%m', occurred_at) AS INTEGER),
                   kind,
                   currency,
                   SUM(CAST(amount AS REAL)) - SUM(CAST(COALESCE(reimbursed_amount, '0') AS REAL))
            FROM transactions
            WHERE voided_at IS NULL
              AND kind IN ('expense', 'income')
              AND occurred_at >= ?1 AND occurred_at < ?2
            GROUP BY 1, 2, kind, currency
            ORDER BY 1, 2
            "#,
        )?;
        let rows = statement.query_map(params![timestamp(start), timestamp(end)], |row| {
            Ok((
                row.get::<_, i32>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;

        // 预填充范围内每个自然月的零点，保证无流水的月份也在序列里。
        let mut by_month: BTreeMap<(i32, u32), (Decimal, Decimal)> = BTreeMap::new();
        for offset in 0..months {
            let index = start_index + offset as i32;
            let year = index.div_euclid(12);
            let month = (index.rem_euclid(12)) as u32 + 1;
            by_month.insert((year, month), (Decimal::ZERO, Decimal::ZERO));
        }

        for row in rows {
            let (year, month, kind, tx_currency, sum) = row?;
            let net = Decimal::from_f64(sum)
                .ok_or_else(|| {
                    KokuError::InvalidInput("invalid monetary aggregate from database".to_owned())
                })?
                .round_dp(4);
            let amount = self.convert_amount(net, &tx_currency, &currency, today)?;
            let entry = by_month
                .entry((year, month))
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            match TransactionKind::from_db(&kind)? {
                TransactionKind::Income => entry.0 += amount,
                TransactionKind::Expense => entry.1 += amount,
                TransactionKind::Transfer
                | TransactionKind::Loan
                | TransactionKind::Adjustment
                | TransactionKind::Trade => {}
            }
        }

        Ok(by_month
            .into_iter()
            .map(|((year, month), (income, expense))| MonthlyTrendPoint {
                year,
                month,
                total_income: income,
                total_expense: expense,
                net: income - expense,
            })
            .collect())
    }

    /// 最近 `months` 个月趋势区间内收支流水涉及的所有币种（供调用方确保折算汇率可用）。
    pub fn trend_currencies(&self, months: u32) -> Result<Vec<String>> {
        let (_, start, end) = trend_bounds(months)?;
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

/// 计算最近 `months` 个自然月的区间：返回（起始月索引、区间起点、区间终点）。
/// 区间终点为下月 0 点，与 `month_bounds` 的开区间语义一致。
fn trend_bounds(months: u32) -> Result<(i32, DateTime<Utc>, DateTime<Utc>)> {
    if !(1..=120).contains(&months) {
        return Err(KokuError::InvalidInput(
            "trend months must be between 1 and 120".to_owned(),
        ));
    }
    let now = Utc::now();
    let end_index = now.year() * 12 + now.month() as i32 - 1;
    let start_index = end_index - (months as i32 - 1);
    let start_year = start_index.div_euclid(12);
    let start_month = (start_index.rem_euclid(12)) as u32 + 1;
    let (start, _) = month_bounds(start_year, start_month)?;
    let (_, end) = month_bounds(now.year(), now.month())?;
    Ok((start_index, start, end))
}
