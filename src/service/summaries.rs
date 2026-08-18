//! 月度/年度/滚动统计：SQL 聚合的收入/支出/现金流汇总，所有币种按汇率折算到显示币种。

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};
use rusqlite::params;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

use super::*;
use crate::domain::{
    CashFlowSummary, CategoryExpense, MonthlySummary, MonthlyTrendPoint, RollingPoint,
    RollingSummary, TagSummary, TransactionKind, YearlySummary,
};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

/// 按分类聚合的公共 FROM 子句：把交易按「有效分类」展开——有拆分
/// （transaction_splits）的交易按拆分行计入，父分类不参与统计；无拆分的
/// 按自身分类计入。父交易金额与拆分同时出现时不双计。
const CATEGORY_AGG_FROM: &str = r#"
FROM transactions t
JOIN categories c ON c.id = t.category_id
LEFT JOIN transaction_splits s ON s.transaction_id = t.id
LEFT JOIN categories cs ON cs.id = s.category_id
"#;

/// 有效分类 id / 名称列表达式（有拆分用拆分行分类，否则用父分类）。
const CATEGORY_AGG_CLASS: &str = r#"
COALESCE(s.category_id, t.category_id) AS category_id,
COALESCE(cs.name, c.name) AS category_name
"#;

/// 净额表达式：有拆分按拆分行金额（父交易已报销额按金额比例分摊扣减），
/// 无拆分按父交易金额 - 已报销额。
const CATEGORY_AGG_AMOUNT: &str = r#"
SUM(CASE WHEN s.id IS NOT NULL
         THEN CAST(s.amount AS REAL)
              - CAST(COALESCE(t.reimbursed_amount, '0') AS REAL)
                * (CAST(s.amount AS REAL) / CAST(t.amount AS REAL))
         ELSE CAST(t.amount AS REAL)
              - CAST(COALESCE(t.reimbursed_amount, '0') AS REAL)
    END)
"#;

/// 分类聚合行的公共结果：(总收入, 总支出, 收入分类明细, 支出分类明细)。
type FlowTotals = (
    Decimal,
    Decimal,
    BTreeMap<(i64, String), Decimal>,
    BTreeMap<(i64, String), Decimal>,
);

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
        let sql = format!(
            r#"
            SELECT t.kind,
                   {class},
                   t.currency,
                   {amount}
            {from}
            WHERE t.voided_at IS NULL
              AND t.kind IN ('expense', 'income')
              AND t.occurred_at >= ?1 AND t.occurred_at < ?2
            GROUP BY 1, 2, 3, 4
            ORDER BY 1, 2, 4
            "#,
            class = CATEGORY_AGG_CLASS,
            amount = CATEGORY_AGG_AMOUNT,
            from = CATEGORY_AGG_FROM,
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![timestamp(start), timestamp(end)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;

        let (total_income, total_expense, income_totals, expense_totals) =
            self.accumulate_flow_rows(rows, &currency, today)?;
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

    /// 标签汇总：同时带有全部指定标签的收支流水，按分类聚合、折算到显示币种。
    /// `year`/`month` 为 `None` 时统计全部历史；两个参数必须同时给出或同时缺省。
    pub fn tag_summary(
        &self,
        tags: &[String],
        year: Option<i32>,
        month: Option<u32>,
        currency: &str,
    ) -> Result<TagSummary> {
        let normalized = normalize_tags(tags)?;
        let currency = normalize_currency(currency.to_owned())?;
        let today = Utc::now().date_naive();
        let range = optional_month_bounds(year, month)?;
        let (filter_sql, params) = tag_filter_sql(&normalized, range.as_ref());
        let sql = format!(
            "SELECT t.kind,
                    {class},
                    t.currency,
                    {amount}
             {from}
             WHERE {filter_sql}
             GROUP BY 1, 2, 3, 4
             ORDER BY 1, 2, 4",
            class = CATEGORY_AGG_CLASS,
            amount = CATEGORY_AGG_AMOUNT,
            from = CATEGORY_AGG_FROM,
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params_from_iter(params.iter().map(|value| value.as_ref())),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4)?,
                ))
            },
        )?;
        let (total_income, total_expense, income_totals, expense_totals) =
            self.accumulate_flow_rows(rows, &currency, today)?;
        Ok(TagSummary {
            tags: normalized,
            year,
            month,
            currency,
            total_income,
            total_expense,
            retained: total_income - total_expense,
            income_sources: cash_flow_items(income_totals, total_income),
            expense_destinations: cash_flow_items(expense_totals, total_expense),
        })
    }

    /// 标签汇总涉及的币种（供调用方确保折算汇率可用），范围与 `tag_summary` 一致。
    pub fn tag_currencies(
        &self,
        tags: &[String],
        year: Option<i32>,
        month: Option<u32>,
    ) -> Result<Vec<String>> {
        let normalized = normalize_tags(tags)?;
        let range = optional_month_bounds(year, month)?;
        let (filter_sql, params) = tag_filter_sql(&normalized, range.as_ref());
        let sql = format!("SELECT DISTINCT t.currency FROM transactions t WHERE {filter_sql}");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(
            rusqlite::params_from_iter(params.iter().map(|value| value.as_ref())),
            |row| row.get::<_, String>(0),
        )?;
        let mut currencies = Vec::new();
        for row in rows {
            currencies.push(row?);
        }
        Ok(currencies)
    }

    /// 分类聚合行的公共折算/累加逻辑（cash_flow_summary 与 tag_summary 共用）。
    fn accumulate_flow_rows(
        &self,
        rows: impl Iterator<Item = rusqlite::Result<(String, i64, String, String, f64)>>,
        currency: &str,
        today: NaiveDate,
    ) -> Result<FlowTotals> {
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
            let amount = self.convert_amount(net, &tx_currency, currency, today)?;
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
                | TransactionKind::Trade
                | TransactionKind::Deposit => {}
            }
        }
        Ok((total_income, total_expense, income_totals, expense_totals))
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
                | TransactionKind::Trade
                | TransactionKind::Deposit => {}
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

    /// 年度汇总：某自然年内逐月收支、全年合计与分类明细，统一折算到显示币种。
    /// 复用月度聚合的折算/取整逻辑；无流水的月份补零。
    pub fn yearly_summary(&self, year: i32, currency: &str) -> Result<YearlySummary> {
        let currency = normalize_currency(currency.to_owned())?;
        let (start, end) = year_bounds(year)?;
        let today = Utc::now().date_naive();
        let sql = format!(
            r#"
            SELECT CAST(strftime('%m', t.occurred_at) AS INTEGER),
                   t.kind,
                   {class},
                   t.currency,
                   {amount}
            {from}
            WHERE t.voided_at IS NULL
              AND t.kind IN ('expense', 'income')
              AND t.occurred_at >= ?1 AND t.occurred_at < ?2
            GROUP BY 1, 2, 3, 4, 5
            ORDER BY 1, 2, 3
            "#,
            class = CATEGORY_AGG_CLASS,
            amount = CATEGORY_AGG_AMOUNT,
            from = CATEGORY_AGG_FROM,
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![timestamp(start), timestamp(end)], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, f64>(5)?,
            ))
        })?;

        let mut total_income = Decimal::ZERO;
        let mut total_expense = Decimal::ZERO;
        let mut income_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        let mut expense_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        let mut month_income: BTreeMap<u32, Decimal> = BTreeMap::new();
        let mut month_expense: BTreeMap<u32, Decimal> = BTreeMap::new();

        for row in rows {
            let (month, kind, category_id, category_name, tx_currency, sum) = row?;
            let net = Decimal::from_f64(sum)
                .ok_or_else(|| {
                    KokuError::InvalidInput("invalid monetary aggregate from database".to_owned())
                })?
                .round_dp(4);
            let amount = self.convert_amount(net, &tx_currency, &currency, today)?;
            match TransactionKind::from_db(&kind)? {
                TransactionKind::Income => {
                    total_income += amount;
                    *month_income.entry(month).or_insert(Decimal::ZERO) += amount;
                    *income_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Expense => {
                    total_expense += amount;
                    *month_expense.entry(month).or_insert(Decimal::ZERO) += amount;
                    *expense_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Transfer
                | TransactionKind::Loan
                | TransactionKind::Adjustment
                | TransactionKind::Trade
                | TransactionKind::Deposit => {}
            }
        }

        // 12 个自然月补零，1 月在前。
        let mut months = Vec::with_capacity(12);
        for month in 1..=12 {
            let income = month_income.get(&month).copied().unwrap_or_default();
            let expense = month_expense.get(&month).copied().unwrap_or_default();
            months.push(MonthlyTrendPoint {
                year,
                month,
                total_income: income,
                total_expense: expense,
                net: income - expense,
            });
        }

        Ok(YearlySummary {
            year,
            currency,
            total_income,
            total_expense,
            net: total_income - total_expense,
            months,
            income_sources: cash_flow_items(income_totals, total_income),
            expense_destinations: cash_flow_items(expense_totals, total_expense),
        })
    }

    /// 某年收支流水涉及的所有币种（供调用方确保折算汇率可用）。
    pub fn yearly_currencies(&self, year: i32) -> Result<Vec<String>> {
        let (start, end) = year_bounds(year)?;
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

    /// 滚动平均：复用 `monthly_trend` 得到最近 `months` 个月的逐月收支，
    /// 再逐月计算截至该月（含）的 trailing window 平均值（窗口内不满时取实际月数）。
    pub fn rolling_summary(
        &self,
        months: u32,
        window: u32,
        currency: &str,
    ) -> Result<RollingSummary> {
        if !(1..=120).contains(&window) {
            return Err(KokuError::InvalidInput(
                "rolling window must be between 1 and 120 months".to_owned(),
            ));
        }
        if window > months {
            return Err(KokuError::InvalidInput(
                "rolling window cannot exceed the number of trend months".to_owned(),
            ));
        }
        let currency = normalize_currency(currency.to_owned())?;
        let trend = self.monthly_trend(months, &currency)?;

        // 用 VecDeque 维护滑动窗口内的逐月净值，避免重复求和。
        let mut window_income: std::collections::VecDeque<Decimal> = Default::default();
        let mut window_expense: std::collections::VecDeque<Decimal> = Default::default();
        let mut sum_income = Decimal::ZERO;
        let mut sum_expense = Decimal::ZERO;

        let mut points = Vec::with_capacity(trend.len());
        for point in &trend {
            window_income.push_back(point.total_income);
            window_expense.push_back(point.total_expense);
            sum_income += point.total_income;
            sum_expense += point.total_expense;
            if window_income.len() > window as usize {
                if let Some(popped) = window_income.pop_front() {
                    sum_income -= popped;
                }
                if let Some(popped) = window_expense.pop_front() {
                    sum_expense -= popped;
                }
            }
            let divisor = Decimal::from(window_income.len());
            let income_avg = (sum_income / divisor).round_dp(2);
            let expense_avg = (sum_expense / divisor).round_dp(2);
            let net_avg = (income_avg - expense_avg).round_dp(2);
            points.push(RollingPoint {
                year: point.year,
                month: point.month,
                income: point.total_income,
                expense: point.total_expense,
                net: point.net,
                income_avg,
                expense_avg,
                net_avg,
            });
        }

        Ok(RollingSummary {
            currency,
            months,
            window,
            points,
        })
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

/// 校验、去重标签名；至少需要一个非空标签。
fn normalize_tags(tags: &[String]) -> Result<Vec<String>> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let trimmed = validate_tag_name(tag)?;
        if seen.insert(trimmed.clone()) {
            normalized.push(trimmed);
        }
    }
    if normalized.is_empty() {
        return Err(KokuError::InvalidInput(
            "at least one tag is required".to_owned(),
        ));
    }
    Ok(normalized)
}

/// 可选的自然月时间范围：两者都给定时返回该月的 [start, end) 时间戳，
/// 两者都缺省时返回 `None`（全部历史）；只给一个则报错。
fn optional_month_bounds(
    year: Option<i32>,
    month: Option<u32>,
) -> Result<Option<(String, String)>> {
    match (year, month) {
        (Some(year), Some(month)) => month_bounds(year, month)
            .map(|(start, end)| (timestamp(start), timestamp(end)))
            .map(Some),
        (None, None) => Ok(None),
        _ => Err(KokuError::InvalidInput(
            "year and month must be provided together".to_owned(),
        )),
    }
}

/// 标签过滤的 WHERE 片段与参数：交易须同时带有全部指定标签（AND 语义）。
/// `range` 给定时追加月份范围条件。
fn tag_filter_sql(
    tags: &[String],
    range: Option<&(String, String)>,
) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let placeholders = vec!["?"; tags.len()].join(",");
    let mut sql = format!(
        "t.voided_at IS NULL AND t.kind IN ('expense', 'income')
         AND t.id IN (
             SELECT tt.transaction_id
             FROM transaction_tags tt
             JOIN tags g ON g.id = tt.tag_id
             WHERE g.name IN ({placeholders})
             GROUP BY tt.transaction_id
             HAVING COUNT(DISTINCT g.name) = ?
         )"
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = tags
        .iter()
        .map(|tag| Box::new(tag.clone()) as Box<dyn rusqlite::ToSql>)
        .collect();
    params.push(Box::new(tags.len() as i64));
    if let Some((start, end)) = range {
        sql.push_str(" AND t.occurred_at >= ? AND t.occurred_at < ?");
        params.push(Box::new(start.clone()));
        params.push(Box::new(end.clone()));
    }
    (sql, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use chrono::NaiveDate;

    fn test_service() -> Result<BookkeepingService> {
        BookkeepingService::in_memory()
    }

    fn date_at(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
    }

    /// 建一个 CNY 账户 + 餐饮支出分类 + 工资收入分类。
    fn seed(service: &mut BookkeepingService) -> Result<(i64, i64, i64)> {
        service.ensure_default_categories()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        let salary = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "工资")
            .unwrap();
        Ok((account.id, food.id, salary.id))
    }

    #[test]
    fn yearly_summary_aggregates_months_totals_and_categories() -> Result<()> {
        let mut service = test_service()?;
        let (account, food, salary) = seed(&mut service)?;
        // 2026-01：收入 5000，支出 800（餐饮）
        service.record_income(
            account,
            salary,
            Decimal::from(5000_u32),
            date_at(2026, 1, 10),
            "工资",
        )?;
        service.record_expense(
            account,
            food,
            Decimal::from(800_u32),
            date_at(2026, 1, 15),
            "聚餐",
        )?;
        // 2026-03：支出 200（餐饮）
        service.record_expense(
            account,
            food,
            Decimal::from(200_u32),
            date_at(2026, 3, 5),
            "外卖",
        )?;
        // 2025-12 的流水不属于 2026 年
        service.record_expense(
            account,
            food,
            Decimal::from(999_u32),
            date_at(2025, 12, 31),
            "去年",
        )?;

        let summary = service.yearly_summary(2026, "CNY")?;
        assert_eq!(summary.year, 2026);
        assert_eq!(summary.total_income, Decimal::from(5000_u32));
        assert_eq!(summary.total_expense, Decimal::from(1000_u32));
        assert_eq!(summary.net, Decimal::from(4000_u32));
        assert_eq!(summary.months.len(), 12);
        assert_eq!(summary.months[0].total_income, Decimal::from(5000_u32));
        assert_eq!(summary.months[0].total_expense, Decimal::from(800_u32));
        assert_eq!(summary.months[1].total_income, Decimal::ZERO);
        assert_eq!(summary.months[2].total_expense, Decimal::from(200_u32));
        // 分类明细：支出只有餐饮 1000；收入只有工资 5000
        assert_eq!(summary.expense_destinations.len(), 1);
        assert_eq!(summary.expense_destinations[0].category_name, "餐饮");
        assert_eq!(
            summary.expense_destinations[0].amount,
            Decimal::from(1000_u32)
        );
        assert_eq!(summary.income_sources.len(), 1);
        assert_eq!(summary.income_sources[0].category_name, "工资");
        Ok(())
    }

    #[test]
    fn yearly_currencies_lists_only_that_years_currencies() -> Result<()> {
        let mut service = test_service()?;
        let (account, food, _) = seed(&mut service)?;
        service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            date_at(2026, 1, 1),
            "今年",
        )?;
        service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            date_at(2025, 1, 1),
            "去年",
        )?;
        assert_eq!(service.yearly_currencies(2026)?.len(), 1);
        assert!(service.yearly_currencies(2026)?.contains(&"CNY".to_owned()));
        Ok(())
    }

    #[test]
    fn rolling_summary_computes_trailing_window_averages() -> Result<()> {
        let mut service = test_service()?;
        let (account, food, salary) = seed(&mut service)?;
        // 最近 4 个月：逐月收入 1000/1000/1000/1000，支出 100/200/300/400
        let now = Utc::now();
        let (end_year, end_month) = (now.year(), now.month());
        let mut index = (end_year * 12 + end_month as i32 - 1) - 4;
        let months: Vec<(i32, u32)> = (0..4)
            .map(|_| {
                index += 1;
                (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
            })
            .collect();
        for (offset, (year, month)) in months.iter().enumerate() {
            service.record_income(
                account,
                salary,
                Decimal::from(1000_u32),
                date_at(*year, *month, 1),
                "工资",
            )?;
            service.record_expense(
                account,
                food,
                Decimal::from(100_u32 * (offset as u32 + 1)),
                date_at(*year, *month, 2),
                "开销",
            )?;
        }

        let summary = service.rolling_summary(4, 3, "CNY")?;
        assert_eq!(summary.window, 3);
        assert_eq!(summary.points.len(), 4);
        // 前 3 个月窗口不满：平均值 = 实际月数
        assert_eq!(summary.points[0].income_avg, Decimal::from(1000_u32));
        assert_eq!(summary.points[0].expense_avg, Decimal::from(100_u32));
        // 第 3 个月起窗口满 3：收入均值恒为 1000
        assert_eq!(summary.points[2].income_avg, Decimal::from(1000_u32));
        // 支出 3 个月均值 = (100+200+300)/3 = 200
        assert_eq!(summary.points[2].expense_avg, Decimal::from(200_u32));
        // 第 4 个月滚动窗口覆盖第 2-4 月：(200+300+400)/3 = 300
        assert_eq!(summary.points[3].expense_avg, Decimal::from(300_u32));
        // 结余均值 = 收入均值 - 支出均值
        assert_eq!(summary.points[3].net_avg, Decimal::from(700_u32),);
        Ok(())
    }

    #[test]
    fn rolling_window_cannot_exceed_trend_or_limits() -> Result<()> {
        let service = test_service()?;
        assert!(service.rolling_summary(2, 3, "CNY").is_err());
        assert!(service.rolling_summary(12, 0, "CNY").is_err());
        assert!(service.rolling_summary(12, 121, "CNY").is_err());
        assert!(service.rolling_summary(12, 1, "CNY").is_ok());
        Ok(())
    }
}
