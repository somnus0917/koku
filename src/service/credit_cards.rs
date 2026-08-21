//! 信用卡账单模型：额度占用、账单快照与出账/未出账金额。
//!
//! # 核心语义
//! - 消费 = Credit 账户上的 Expense（正常支出）；
//! - 还款 = 储蓄账户 → Credit 账户的 Transfer（**不是** Expense，避免重复统计）；
//! - 已用额度以**账户余额**为准（Credit 余额 = 未还欠款，由现有 `AccountType`
//!   语义维护：支出增、还款减；期初余额、余额调整、收入/退款等合法操作一并计入），
//!   `used_credit = max(0, account.balance)`，溢缴（余额为负）按 0 计。
//!
//! # 账单口径
//! - 消费：该账户上未撤销的 Expense，按 `settled_amount`（账户币种结算额）计，
//!   金额以 TEXT 从 SQLite 原样读出后用 `Decimal` 在 Rust 中精确求和
//!   （全程不使用 REAL/f64 做账务计算，如 0.10+0.20+0.30 精确等于 0.60）；
//! - 账单周期：`(上一账单日, 最近账单日]`，即 `[上一账单日次日 00:00,
//!   最近账单日次日 00:00)`——账单日当天的消费计入本期已出账；
//! - 每个已经结束的账单周期首次被读取时，都会固化为不可变账单快照；
//! - `current_statement_amount` = 最近一期已出账快照中未被冲抵的消费；
//! - `unbilled_amount` = 最近账单日次日 00:00 之后、截至 as_of 的未出账消费；
//! - 冲抵口径：`tracked = old + current + unbilled`，
//!   `tracked_unpaid = min(used_credit, tracked)`，差额 `tracked − tracked_unpaid`
//!   视为可归因的还款/贷项，按 FIFO（最早周期 → 最近周期）依次冲抵；
//!   `used_credit > tracked` 的多出部分视为历史/未分类负债，不伪造进
//!   current/unbilled（二者之和恒不超过真实 used_credit）。
//!
//! 本版不追踪「某次还款具体偿还哪一期」，采用上述近似并在 README 中说明；
//! 不做最低还款/分期/利息/罚息等结算引擎。
//!
//! 所有日期 helper 显式接收日期参数，不内嵌 `Utc::now()`，保证可测试。

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use rusqlite::{params_from_iter, types::Value};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{AccountType, CreditCardStatement, CreditCardSummary};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 计算信用卡账单摘要（`as_of` 为快照时间点；日期 helper 均显式传参）。
    ///
    /// 仅对 Credit 账户有效；非 Credit 账户返回明确错误。`statement_day` /
    /// `due_day` 未设置时返回部分字段为 `None` 的部分摘要（不 panic）。
    pub fn credit_card_summary(
        &mut self,
        account_id: i64,
        as_of: DateTime<Utc>,
    ) -> Result<CreditCardSummary> {
        let account = self.account(account_id)?;
        if account.account_type != AccountType::Credit {
            return Err(KokuError::InvalidInput(format!(
                "credit card summary is only available for credit accounts ({} is {})",
                account.name,
                account.account_type.as_str()
            )));
        }
        // 已用额度以账户实际余额为准（Credit 余额 = 未还欠款；溢缴为负 → 0）。
        let used_credit = account.balance.max(Decimal::ZERO);
        let (current_statement_amount, unbilled_amount) = match account.statement_day {
            Some(day) => {
                let as_of_date = as_of.date_naive();
                let recent = recent_statement_date(as_of_date, day);
                self.sync_credit_card_statements(&account, recent, as_of)?;
                let statements = self.credit_card_statements(account_id)?;
                let old = statements
                    .iter()
                    .filter(|item| item.statement_date < recent)
                    .map(|item| item.amount)
                    .sum();
                let current = statements
                    .iter()
                    .find(|item| item.statement_date == recent)
                    .map_or(Decimal::ZERO, |item| item.amount);
                let current_end = midnight_utc(recent + Duration::days(1));
                let unbilled = self.sum_expenses(account_id, &as_of, Some(&current_end), None)?;
                let (current_unpaid, unbilled_unpaid) =
                    apply_fifo_cap(old, current, unbilled, used_credit);
                (Some(current_unpaid), Some(unbilled_unpaid))
            }
            None => (None, None),
        };
        let as_of_date = as_of.date_naive();
        let available_credit = account.credit_limit.map(|limit| limit - used_credit);
        Ok(CreditCardSummary {
            account_id,
            currency: account.currency,
            credit_limit: account.credit_limit,
            used_credit,
            available_credit,
            statement_day: account.statement_day,
            due_day: account.due_day,
            current_statement_amount,
            unbilled_amount,
            next_statement_date: account
                .statement_day
                .map(|day| next_statement_date(as_of_date, day)),
            next_due_date: account.due_day.map(|day| next_due_date(as_of_date, day)),
        })
    }

    /// 账户上未撤销 Expense 的 `settled_amount` 之和（可加下/上界过滤）。
    ///
    /// 金额以 TEXT 从 SQLite 原样读出，用 `Decimal` 在 Rust 中精确求和——
    /// 全程不使用 REAL/f64 做账务计算，杜绝浮点误差。
    fn sum_expenses(
        &self,
        account_id: i64,
        as_of: &DateTime<Utc>,
        lower: Option<&DateTime<Utc>>,
        upper: Option<&DateTime<Utc>>,
    ) -> Result<Decimal> {
        let mut sql = String::from(
            "SELECT settled_amount FROM transactions \
             WHERE account_id = ? AND kind = 'expense' AND voided_at IS NULL AND occurred_at <= ?",
        );
        let mut params: Vec<Value> =
            vec![Value::Integer(account_id), Value::Text(timestamp(*as_of))];
        if let Some(lower) = lower {
            sql.push_str(" AND occurred_at >= ?");
            params.push(Value::Text(timestamp(*lower)));
        }
        if let Some(upper) = upper {
            sql.push_str(" AND occurred_at < ?");
            params.push(Value::Text(timestamp(*upper)));
        }
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(params), |row| row.get::<_, String>(0))?;
        let mut total = Decimal::ZERO;
        for row in rows {
            total += decimal_from_db(&row?)?;
        }
        Ok(total)
    }

    /// 把所有已经结束、尚未快照的信用卡账单周期固化下来。
    ///
    /// 快照只会 `INSERT OR IGNORE` 一次；之后即使录入一笔追溯日期的交易，也
    /// 不会悄悄改写历史账单。这符合对账单可追溯性的预期。
    fn sync_credit_card_statements(
        &mut self,
        account: &crate::domain::Account,
        recent_statement: NaiveDate,
        as_of: DateTime<Utc>,
    ) -> Result<()> {
        let Some(statement_day) = account.statement_day else {
            return Ok(());
        };
        let first_expense: Option<String> = self.conn.query_row(
            "SELECT MIN(occurred_at) FROM transactions
             WHERE account_id = ?1 AND kind = 'expense' AND voided_at IS NULL AND occurred_at <= ?2",
            rusqlite::params![account.id, timestamp(as_of)],
            |row| row.get(0),
        )?;
        let Some(first_expense) = first_expense else {
            return Ok(());
        };
        let first_date = parse_timestamp(&first_expense)?.date_naive();
        let mut statement_date = statement_on_or_after(first_date, statement_day);
        while statement_date <= recent_statement {
            let previous = previous_statement_date(statement_date, statement_day);
            let start = midnight_utc(previous + Duration::days(1));
            let end = midnight_utc(statement_date + Duration::days(1));
            let amount = self.sum_expenses(account.id, &as_of, Some(&start), Some(&end))?;
            let due_at = account.due_day.map(|due_day| {
                timestamp(midnight_utc(due_date_for_statement(
                    statement_date,
                    due_day,
                )))
            });
            self.conn.execute(
                "INSERT OR IGNORE INTO credit_card_statements
                 (account_id, statement_date, due_at, amount, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    account.id,
                    statement_date.to_string(),
                    due_at,
                    amount.to_string(),
                    timestamp(as_of),
                ],
            )?;
            statement_date = next_statement_date(statement_date, statement_day);
        }
        Ok(())
    }

    fn credit_card_statements(&self, account_id: i64) -> Result<Vec<StoredCreditCardStatement>> {
        let mut statement = self.conn.prepare(
            "SELECT statement_date, due_at, amount
             FROM credit_card_statements WHERE account_id = ?1 ORDER BY statement_date",
        )?;
        let rows = statement.query_map([account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (statement_date, due_at, amount) = row?;
            Ok(StoredCreditCardStatement {
                statement_date: NaiveDate::parse_from_str(&statement_date, "%Y-%m-%d").map_err(
                    |error| {
                        KokuError::InvalidInput(format!(
                            "invalid credit card statement date {statement_date}: {error}"
                        ))
                    },
                )?,
                due_at: due_at.as_deref().map(parse_timestamp).transpose()?,
                amount: decimal_from_db(&amount)?,
            })
        })
        .collect()
    }

    /// 读取账单历史，同时给出以当前账户余额按 FIFO 分摊的未还金额。
    pub fn credit_card_statements_history(
        &mut self,
        account_id: i64,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<CreditCardStatement>> {
        let account = self.account(account_id)?;
        if account.account_type != AccountType::Credit {
            return Err(KokuError::InvalidInput(
                "credit card statements are only available for credit accounts".to_owned(),
            ));
        }
        if let Some(day) = account.statement_day {
            self.sync_credit_card_statements(
                &account,
                recent_statement_date(as_of.date_naive(), day),
                as_of,
            )?;
        }
        let statements = self.credit_card_statements(account_id)?;
        let tracked: Decimal = statements.iter().map(|item| item.amount).sum();
        let mut paid_or_credited = tracked - account.balance.max(Decimal::ZERO).min(tracked);
        let mut history = Vec::with_capacity(statements.len());
        for item in statements {
            let outstanding = (item.amount - paid_or_credited).max(Decimal::ZERO);
            paid_or_credited = (paid_or_credited - item.amount).max(Decimal::ZERO);
            history.push(CreditCardStatement {
                statement_date: item.statement_date,
                due_at: item.due_at,
                amount: item.amount,
                outstanding,
            });
        }
        history.reverse();
        Ok(history)
    }

    /// 为到期提醒准备所有信用卡的账单快照，再返回按账期排序的未还金额。
    pub(super) fn due_credit_card_statements(
        &mut self,
        now: DateTime<Utc>,
        horizon: DateTime<Utc>,
    ) -> Result<Vec<CreditCardStatementReminder>> {
        let accounts = self.accounts()?;
        let mut reminders = Vec::new();
        for account in accounts
            .into_iter()
            .filter(|account| account.account_type == AccountType::Credit)
        {
            let Some(statement_day) = account.statement_day else {
                continue;
            };
            self.sync_credit_card_statements(
                &account,
                recent_statement_date(now.date_naive(), statement_day),
                now,
            )?;
            let statements = self.credit_card_statements(account.id)?;
            let tracked: Decimal = statements.iter().map(|item| item.amount).sum();
            let mut paid_or_credited = tracked - account.balance.max(Decimal::ZERO).min(tracked);
            for item in statements {
                let outstanding = (item.amount - paid_or_credited).max(Decimal::ZERO);
                paid_or_credited = (paid_or_credited - item.amount).max(Decimal::ZERO);
                if let Some(due_at) = item.due_at.filter(|due_at| *due_at <= horizon) {
                    if outstanding > Decimal::ZERO {
                        reminders.push(CreditCardStatementReminder {
                            account_id: account.id,
                            account_name: account.name.clone(),
                            amount: outstanding,
                            currency: account.currency.clone(),
                            due_at,
                        });
                    }
                }
            }
        }
        Ok(reminders)
    }
}

#[derive(Debug, Clone)]
struct StoredCreditCardStatement {
    statement_date: NaiveDate,
    due_at: Option<DateTime<Utc>>,
    amount: Decimal,
}

#[derive(Debug, Clone)]
pub(super) struct CreditCardStatementReminder {
    pub(super) account_id: i64,
    pub(super) account_name: String,
    pub(super) amount: Decimal,
    pub(super) currency: String,
    pub(super) due_at: DateTime<Utc>,
}

/// FIFO + 上限口径：把「可归因的还款/贷项」按 最早桶 → 最近桶 依次冲抵，
/// 并保证 `current + unbilled` 之和不超过真实 `used_credit`。
///
/// - `old`：最近账单周期之前的消费；`current`：最近已出账周期消费；
///   `unbilled`：最近账单日之后未出账消费；
/// - `used_credit` = `max(0, 账户余额)`（账户余额的最终真值）；
/// - `tracked = old + current + unbilled`，`tracked_unpaid = min(used, tracked)`，
///   `effective = tracked − tracked_unpaid`（还款/收入/退款等已冲抵部分）；
/// - 若 `used > tracked`，多出的部分视为历史/未分类负债，不进 current/unbilled。
fn apply_fifo_cap(
    old: Decimal,
    current: Decimal,
    unbilled: Decimal,
    used_credit: Decimal,
) -> (Decimal, Decimal) {
    let tracked = old + current + unbilled;
    let tracked_unpaid = used_credit.min(tracked);
    let effective = tracked - tracked_unpaid;
    let mut remaining = effective;
    let old_unpaid = (old - remaining).max(Decimal::ZERO);
    remaining = (remaining - old).max(Decimal::ZERO);
    let current_unpaid = (current - remaining).max(Decimal::ZERO);
    remaining = (remaining - current).max(Decimal::ZERO);
    let unbilled_unpaid = (unbilled - remaining).max(Decimal::ZERO);
    debug_assert_eq!(
        old_unpaid + current_unpaid + unbilled_unpaid,
        tracked_unpaid
    );
    (current_unpaid, unbilled_unpaid)
}

// ---------------------------------------------------------------------------
// 账单日期 helper（纯函数，显式传参，不内嵌 now）
// ---------------------------------------------------------------------------

/// 某年某月的天数（2 月按闰年）。
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        // 防御分支：月份来自 NaiveDate，恒为 1..=12。
        _ => 30,
    }
}

/// 按月安全生成「账单日」：`day` 超出当月天数时取当月最后一天
/// （如 31 日在 2 月 → 2 月末），并把入参钳制到 1..=31 保证不 panic。
fn day_in_month(year: i32, month: u32, day: u32) -> NaiveDate {
    let day = day.clamp(1, 31).min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).expect("day clamped into valid month days")
}

fn previous_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn midnight_utc(date: NaiveDate) -> DateTime<Utc> {
    date.and_time(NaiveTime::MIN).and_utc()
}

/// 账单日所在月若不早于交易日，返回该月账单日；否则返回下月账单日。
fn statement_on_or_after(date: NaiveDate, statement_day: u32) -> NaiveDate {
    let current = day_in_month(date.year(), date.month(), statement_day);
    if current >= date {
        current
    } else {
        next_statement_date(date, statement_day)
    }
}

/// 最近一次（`<= as_of`）的账单日。
pub(super) fn recent_statement_date(as_of: NaiveDate, statement_day: u32) -> NaiveDate {
    let current = day_in_month(as_of.year(), as_of.month(), statement_day);
    if current <= as_of {
        current
    } else {
        let (year, month) = previous_month(as_of.year(), as_of.month());
        day_in_month(year, month, statement_day)
    }
}

/// 最近账单日的前一期账单日。
pub(super) fn previous_statement_date(recent: NaiveDate, statement_day: u32) -> NaiveDate {
    let (year, month) = previous_month(recent.year(), recent.month());
    day_in_month(year, month, statement_day)
}

/// 下一次（`> as_of`）的账单日。
pub(super) fn next_statement_date(as_of: NaiveDate, statement_day: u32) -> NaiveDate {
    let current = day_in_month(as_of.year(), as_of.month(), statement_day);
    if current > as_of {
        current
    } else {
        let (year, month) = next_month(as_of.year(), as_of.month());
        day_in_month(year, month, statement_day)
    }
}

/// 某期账单的还款日：严格晚于账单日的最早 due_day 日期（跨月或月底回退）。
///
/// 摘要中的 `next_due_date` 使用 [`next_due_date`]。
pub(super) fn due_date_for_statement(statement: NaiveDate, due_day: u32) -> NaiveDate {
    let current = day_in_month(statement.year(), statement.month(), due_day);
    if current > statement {
        current
    } else {
        let (year, month) = next_month(statement.year(), statement.month());
        day_in_month(year, month, due_day)
    }
}

/// 下一次还款日：`as_of` 之后（严格）最早的 due_day 日期（始终为未来）。
pub(super) fn next_due_date(as_of: NaiveDate, due_day: u32) -> NaiveDate {
    let current = day_in_month(as_of.year(), as_of.month(), due_day);
    if current > as_of {
        current
    } else {
        let (year, month) = next_month(as_of.year(), as_of.month());
        day_in_month(year, month, due_day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CategoryKind, TransactionKind};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn as_of(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        midnight_utc(date(y, m, d))
    }

    /// 带时刻的时间点（边界测试用）。
    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        date(y, m, d)
            .and_hms_opt(h, min, 0)
            .expect("valid clock time")
            .and_utc()
    }

    /// 建一个带默认分类的账本，返回 (service, credit, cash, 餐饮分类 id)。
    fn seeded() -> Result<(BookkeepingService, crate::domain::Account, i64, i64)> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let credit =
            service.create_account("招商 Visa", AccountType::Credit, "CNY", Decimal::ZERO)?;
        let cash = service.create_account(
            "招商储蓄卡",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap()
            .id;
        Ok((service, credit, cash.id, food))
    }

    fn expense(
        service: &mut BookkeepingService,
        account_id: i64,
        category_id: i64,
        amount: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<i64> {
        let tx = service.record_expense(
            account_id,
            category_id,
            Decimal::from_str_exact(amount).unwrap(),
            occurred_at,
            "消费",
        )?;
        Ok(tx.id)
    }

    fn repay(
        service: &mut BookkeepingService,
        from: i64,
        to: i64,
        amount: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<i64> {
        let tx = service.record_transfer(
            from,
            to,
            Decimal::from_str_exact(amount).unwrap(),
            Decimal::from_str_exact(amount).unwrap(),
            occurred_at,
            "还款",
        )?;
        Ok(tx.id)
    }

    // ---------------------------------------------------------------
    // 账户字段
    // ---------------------------------------------------------------

    #[test]
    fn statement_and_due_day_round_trip() -> Result<()> {
        let (mut service, credit, _, _) = seeded()?;
        let account = service.set_statement_day(credit.id, Some(10))?;
        assert_eq!(account.statement_day, Some(10));
        let account = service.set_due_day(credit.id, Some(25))?;
        assert_eq!(account.due_day, Some(25));
        // 清除
        let account = service.set_statement_day(credit.id, None)?;
        assert_eq!(account.statement_day, None);
        assert_eq!(account.due_day, Some(25));
        Ok(())
    }

    #[test]
    fn statement_day_validation_rejects_out_of_range() -> Result<()> {
        let (mut service, credit, _, _) = seeded()?;
        assert!(service.set_statement_day(credit.id, Some(0)).is_err());
        assert!(service.set_statement_day(credit.id, Some(32)).is_err());
        assert!(service.set_due_day(credit.id, Some(0)).is_err());
        assert!(service.set_due_day(credit.id, Some(32)).is_err());
        // 合法值仍可设置。
        service.set_statement_day(credit.id, Some(1))?;
        service.set_due_day(credit.id, Some(31))?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // 摘要入口
    // ---------------------------------------------------------------

    #[test]
    fn summary_is_rejected_for_non_credit_account() -> Result<()> {
        let (mut service, _, cash, _) = seeded()?;
        let error = service.credit_card_summary(cash, as_of(2026, 8, 18));
        assert!(error.is_err());
        let message = format!("{error:?}");
        assert!(message.contains("credit accounts"), "unexpected: {message}");
        Ok(())
    }

    #[test]
    fn summary_without_statement_day_is_partial() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_credit_limit(credit.id, Some(Decimal::from(20000_u32)))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.current_statement_amount, None);
        assert_eq!(summary.unbilled_amount, None);
        assert_eq!(summary.next_statement_date, None);
        assert_eq!(summary.used_credit, Decimal::from(300_u32));
        assert_eq!(summary.credit_limit, Some(Decimal::from(20000_u32)));
        assert_eq!(summary.available_credit, Some(Decimal::from(19700_u32)));
        Ok(())
    }

    // ---------------------------------------------------------------
    // 账单日期 helper
    // ---------------------------------------------------------------

    #[test]
    fn recent_and_next_statement_dates() -> Result<()> {
        // as_of 2026-08-18，账单日 10 → 最近 08-10、下一 09-10。
        assert_eq!(
            recent_statement_date(date(2026, 8, 18), 10),
            date(2026, 8, 10)
        );
        assert_eq!(
            next_statement_date(date(2026, 8, 18), 10),
            date(2026, 9, 10)
        );
        // 账单日当天：最近即当天，下一为次月。
        assert_eq!(
            recent_statement_date(date(2026, 8, 10), 10),
            date(2026, 8, 10)
        );
        assert_eq!(
            next_statement_date(date(2026, 8, 10), 10),
            date(2026, 9, 10)
        );
        // 跨年。
        assert_eq!(
            recent_statement_date(date(2026, 1, 5), 20),
            date(2025, 12, 20)
        );
        assert_eq!(
            next_statement_date(date(2026, 12, 25), 20),
            date(2027, 1, 20)
        );
        // 上一期。
        assert_eq!(
            previous_statement_date(date(2026, 8, 10), 10),
            date(2026, 7, 10)
        );
        Ok(())
    }

    #[test]
    fn statement_day_31_falls_back_to_month_end() -> Result<()> {
        // 2 月无 31 日 → 2 月末（2026 非闰年 → 28 日）。
        assert_eq!(day_in_month(2026, 2, 31), date(2026, 2, 28));
        assert_eq!(day_in_month(2024, 2, 31), date(2024, 2, 29)); // 闰年
        assert_eq!(day_in_month(2026, 4, 31), date(2026, 4, 30));
        // 最近/下一账单日均落到有效日期。
        assert_eq!(
            recent_statement_date(date(2026, 2, 15), 31),
            date(2026, 1, 31)
        );
        assert_eq!(
            next_statement_date(date(2026, 2, 15), 31),
            date(2026, 2, 28)
        );
        assert_eq!(
            next_statement_date(date(2026, 2, 28), 31),
            date(2026, 3, 31)
        );
        Ok(())
    }

    #[test]
    fn due_after_statement_in_same_month() -> Result<()> {
        // statement_day=10, due_day=25：2026-08-10 出账 → 2026-08-25 到期。
        assert_eq!(
            due_date_for_statement(date(2026, 8, 10), 25),
            date(2026, 8, 25)
        );
        assert_eq!(next_due_date(date(2026, 8, 18), 25), date(2026, 8, 25));
        Ok(())
    }

    #[test]
    fn due_before_statement_rolls_to_next_month() -> Result<()> {
        // statement_day=25, due_day=10：2026-08-25 出账 → 2026-09-10 到期。
        assert_eq!(
            due_date_for_statement(date(2026, 8, 25), 10),
            date(2026, 9, 10)
        );
        // 还款日跨年。
        assert_eq!(
            due_date_for_statement(date(2026, 12, 25), 10),
            date(2027, 1, 10)
        );
        Ok(())
    }

    #[test]
    fn due_day_respects_month_end_fallback() -> Result<()> {
        // 1 月 31 日出账、还款日 31：2 月无 31 日 → 2 月末。
        assert_eq!(
            due_date_for_statement(date(2026, 1, 31), 31),
            date(2026, 2, 28)
        );
        // 账单日 31（2 月末）与还款日 31 → 3 月 31。
        assert_eq!(
            due_date_for_statement(date(2026, 2, 28), 31),
            date(2026, 3, 31)
        );
        Ok(())
    }

    // ---------------------------------------------------------------
    // 额度与账单金额
    // ---------------------------------------------------------------

    #[test]
    fn expense_increases_used_credit() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_credit_limit(credit.id, Some(Decimal::from(20000_u32)))?;
        service.set_statement_day(credit.id, Some(10))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::from(300_u32));
        assert_eq!(summary.available_credit, Some(Decimal::from(19700_u32)));
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(300_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        Ok(())
    }

    #[test]
    fn repayment_transfer_reduces_used_credit() -> Result<()> {
        let (mut service, credit, cash, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        repay(&mut service, cash, credit.id, "300.00", as_of(2026, 8, 12))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::ZERO);
        assert_eq!(summary.current_statement_amount, Some(Decimal::ZERO));
        Ok(())
    }

    #[test]
    fn repayment_transfer_is_not_expense() -> Result<()> {
        let (mut service, credit, cash, food) = seeded()?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        repay(&mut service, cash, credit.id, "300.00", as_of(2026, 8, 12))?;
        // 还款是 Transfer：月度支出仍只有 300，绝不变成 600。
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.total_expense, Decimal::from(300_u32));
        Ok(())
    }

    #[test]
    fn voided_expense_restores_credit() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        let tx = expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        let before = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(before.used_credit, Decimal::from(300_u32));
        service.void_transaction(tx)?;
        let after = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(after.used_credit, Decimal::ZERO);
        assert_eq!(after.current_statement_amount, Some(Decimal::ZERO));
        Ok(())
    }

    #[test]
    fn statement_and_unbilled_split_by_statement_day() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        // 08-05 → 上一周期 [07-10, 08-10) → 已出账；08-15 → 未出账。
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        expense(&mut service, credit.id, food, "200.00", as_of(2026, 8, 15))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(300_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::from(200_u32)));
        assert_eq!(summary.used_credit, Decimal::from(500_u32));
        assert_eq!(summary.next_statement_date, Some(date(2026, 9, 10)));
        assert_eq!(summary.next_due_date, None); // 未设还款日
        Ok(())
    }

    #[test]
    fn repayments_fifo_pay_oldest_buckets_first() -> Result<()> {
        let (mut service, credit, cash, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        // 06-20 消费 100（最早，位于 [上一账单日前]）；08-05 消费 300（已出账周期）。
        expense(&mut service, credit.id, food, "100.00", as_of(2026, 6, 20))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        // 还款 150：FIFO 先冲抵 06-20 的 100，剩余 50 冲抵已出账的 300。
        repay(&mut service, cash, credit.id, "150.00", as_of(2026, 8, 12))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        // 已出账 = 300 - 50 = 250；未出账 = 0；已用 = 100 + 300 - 150 = 250。
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(250_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        assert_eq!(summary.used_credit, Decimal::from(250_u32));
        Ok(())
    }

    #[test]
    fn foreign_currency_expense_uses_settled_amount() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        // CNY 信用卡上的一笔 EUR 消费：amount=100 EUR，settled=780 CNY。
        service.record_expense_in_currency(
            credit.id,
            food,
            Decimal::from(100_u32),
            "EUR",
            Decimal::from(780_u32),
            as_of(2026, 8, 5),
            "海外消费",
        )?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        // 额度占用按账户币种结算额（780 CNY），而不是原币 100。
        assert_eq!(summary.used_credit, Decimal::from(780_u32));
        assert_eq!(summary.currency, "CNY");
        // 账户余额按负债语义同步增加（结算额）。
        assert_eq!(service.account(credit.id)?.balance, Decimal::from(780_u32));
        Ok(())
    }

    #[test]
    fn full_summary_shape_with_limit_and_days() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_credit_limit(credit.id, Some(Decimal::from(20000_u32)))?;
        service.set_statement_day(credit.id, Some(10))?;
        service.set_due_day(credit.id, Some(25))?;
        expense(&mut service, credit.id, food, "1230.00", as_of(2026, 8, 5))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.credit_limit, Some(Decimal::from(20000_u32)));
        assert_eq!(summary.used_credit, Decimal::from(1230_u32));
        assert_eq!(summary.available_credit, Some(Decimal::from(18770_u32)));
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(1230_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        assert_eq!(summary.statement_day, Some(10));
        assert_eq!(summary.due_day, Some(25));
        assert_eq!(summary.next_statement_date, Some(date(2026, 9, 10)));
        assert_eq!(summary.next_due_date, Some(date(2026, 8, 25)));
        Ok(())
    }

    // ---------------------------------------------------------------
    // 交易类型合法性回归
    // ---------------------------------------------------------------

    #[test]
    fn credit_expense_and_repayment_transfer_are_the_supported_flow() -> Result<()> {
        let (mut service, credit, cash, food) = seeded()?;
        // 信用卡消费是 Expense。
        let expense_tx = service.record_expense(
            credit.id,
            food,
            Decimal::from(300_u32),
            as_of(2026, 8, 5),
            "餐饮",
        )?;
        assert_eq!(expense_tx.kind, TransactionKind::Expense);
        // 还款是 Transfer（储蓄 → 信用卡）。
        let transfer_tx = service.record_transfer(
            cash,
            credit.id,
            Decimal::from(300_u32),
            Decimal::from(300_u32),
            as_of(2026, 8, 12),
            "还款",
        )?;
        assert_eq!(transfer_tx.kind, TransactionKind::Transfer);
        assert_eq!(transfer_tx.to_account_id, Some(credit.id));
        Ok(())
    }

    #[test]
    fn categories_still_require_matching_kind() -> Result<()> {
        let (mut service, credit, _, _) = seeded()?;
        let income = service
            .categories()?
            .into_iter()
            .find(|item| item.kind == CategoryKind::Income)
            .unwrap()
            .id;
        // 信用卡账户上的收入分类不能用于支出（既有校验不变）。
        assert!(service
            .record_expense(
                credit.id,
                income,
                Decimal::from(10_u32),
                as_of(2026, 8, 5),
                "错"
            )
            .is_err());
        Ok(())
    }

    // ---------------------------------------------------------------
    // 精确小数求和（全程 Decimal，无 REAL/f64）
    // ---------------------------------------------------------------

    #[test]
    fn decimal_sums_are_exact() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        // 0.10 + 0.20 + 0.30 必须精确等于 0.60（浮点求和的 0.30000000000000004
        // 一类误差绝不允许出现）。
        expense(&mut service, credit.id, food, "0.10", as_of(2026, 8, 5))?;
        expense(&mut service, credit.id, food, "0.20", as_of(2026, 8, 6))?;
        expense(&mut service, credit.id, food, "0.30", as_of(2026, 8, 7))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        let exact = Decimal::from_str_exact("0.60")?;
        assert_eq!(summary.used_credit, exact);
        assert_eq!(summary.current_statement_amount, Some(exact));
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        Ok(())
    }

    // ---------------------------------------------------------------
    // 账单日当天的周期归属（(上一账单日, 最近账单日]）
    // ---------------------------------------------------------------

    #[test]
    fn statement_day_boundary_includes_statement_day() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        // 08-09 23:00 / 08-10 00:00 / 08-10 23:30 → 本期已出账（含账单日全天）；
        // 08-11 00:00 → 未出账。
        expense(
            &mut service,
            credit.id,
            food,
            "60.00",
            at(2026, 8, 9, 23, 0),
        )?;
        expense(
            &mut service,
            credit.id,
            food,
            "70.00",
            at(2026, 8, 10, 0, 0),
        )?;
        expense(
            &mut service,
            credit.id,
            food,
            "80.00",
            at(2026, 8, 10, 23, 30),
        )?;
        expense(
            &mut service,
            credit.id,
            food,
            "90.00",
            at(2026, 8, 11, 0, 0),
        )?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(210_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::from(90_u32)));
        assert_eq!(summary.used_credit, Decimal::from(300_u32));
        Ok(())
    }

    #[test]
    fn statement_day_31_february_boundary() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(31))?;
        // 2 月账单日落到 02-28：02-28 23:00 计入本期已出账，03-01 00:00 起未出账。
        expense(
            &mut service,
            credit.id,
            food,
            "100.00",
            at(2026, 2, 28, 23, 0),
        )?;
        expense(&mut service, credit.id, food, "50.00", at(2026, 3, 1, 0, 0))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 3, 10))?;
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(100_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::from(50_u32)));
        Ok(())
    }

    // ---------------------------------------------------------------
    // used_credit 以账户余额为准（max(0, balance)）
    // ---------------------------------------------------------------

    #[test]
    fn opening_balance_counts_toward_used_credit() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let credit = service.create_account(
            "招商 Visa",
            AccountType::Credit,
            "CNY",
            Decimal::from(500_u32),
        )?;
        service.set_statement_day(credit.id, Some(10))?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        // 期初余额 500 直接计入已用额度；无交易 → 出账/未出账为 0。
        assert_eq!(summary.used_credit, Decimal::from(500_u32));
        assert_eq!(summary.current_statement_amount, Some(Decimal::ZERO));
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        Ok(())
    }

    #[test]
    fn balance_adjustment_follows_used_credit() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let credit = service.create_account(
            "招商 Visa",
            AccountType::Credit,
            "CNY",
            Decimal::from(500_u32),
        )?;
        service.adjust_balance(credit.id, Decimal::from(200_u32), "额度修正")?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::from(700_u32));
        // 反向调整回零。
        service.adjust_balance(credit.id, Decimal::from(-700), "修正")?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::ZERO);
        Ok(())
    }

    #[test]
    fn income_refund_reduces_used_credit() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let credit = service.create_account(
            "招商 Visa",
            AccountType::Credit,
            "CNY",
            Decimal::from(500_u32),
        )?;
        let refund = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "退款")
            .unwrap()
            .id;
        // 退款收入入信用卡账户 → 负债减少 → 已用额度同步下降。
        service.record_income(
            credit.id,
            refund,
            Decimal::from(100_u32),
            as_of(2026, 8, 5),
            "退款",
        )?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::from(400_u32));
        Ok(())
    }

    #[test]
    fn overpayment_yields_zero_used_credit() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let credit = service.create_account(
            "招商 Visa",
            AccountType::Credit,
            "CNY",
            Decimal::from(500_u32),
        )?;
        service.set_credit_limit(credit.id, Some(Decimal::from(20000_u32)))?;
        let refund = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "退款")
            .unwrap()
            .id;
        service.record_income(
            credit.id,
            refund,
            Decimal::from(600_u32),
            as_of(2026, 8, 5),
            "退款",
        )?;
        // 溢缴：余额 -100 → used_credit = 0，可用额度 = 全额。
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(service.account(credit.id)?.balance, Decimal::from(-100));
        assert_eq!(summary.used_credit, Decimal::ZERO);
        assert_eq!(summary.available_credit, Some(Decimal::from(20000_u32)));
        Ok(())
    }

    #[test]
    fn statement_buckets_never_exceed_used_credit() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        let refund = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "退款")
            .unwrap()
            .id;
        // 消费 300 + 退款 200 → 余额 100；出账/未出账之和不得超过 used_credit=100。
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        service.record_income(
            credit.id,
            refund,
            Decimal::from(200_u32),
            as_of(2026, 8, 8),
            "退款",
        )?;
        let summary = service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(summary.used_credit, Decimal::from(100_u32));
        assert_eq!(
            summary.current_statement_amount,
            Some(Decimal::from(100_u32))
        );
        assert_eq!(summary.unbilled_amount, Some(Decimal::ZERO));
        Ok(())
    }

    #[test]
    fn closed_statement_snapshot_is_not_rewritten_by_late_entries() -> Result<()> {
        let (mut service, credit, _, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;

        let initial: String = service.conn.query_row(
            "SELECT amount FROM credit_card_statements WHERE account_id = ?1 AND statement_date = '2026-08-10'",
            [credit.id],
            |row| row.get(0),
        )?;
        assert_eq!(initial, "300");

        // 账单已经固化后录入的追溯交易不能悄悄改写历史账单。
        expense(&mut service, credit.id, food, "100.00", as_of(2026, 8, 7))?;
        service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        let persisted: String = service.conn.query_row(
            "SELECT amount FROM credit_card_statements WHERE account_id = ?1 AND statement_date = '2026-08-10'",
            [credit.id],
            |row| row.get(0),
        )?;
        assert_eq!(persisted, "300");
        Ok(())
    }

    #[test]
    fn statement_history_exposes_fifo_outstanding_amounts() -> Result<()> {
        let (mut service, credit, cash, food) = seeded()?;
        service.set_statement_day(credit.id, Some(10))?;
        service.set_due_day(credit.id, Some(25))?;
        expense(&mut service, credit.id, food, "300.00", as_of(2026, 8, 5))?;
        service.credit_card_summary(credit.id, as_of(2026, 8, 18))?;
        repay(&mut service, cash, credit.id, "100.00", as_of(2026, 8, 18))?;

        let statements = service.credit_card_statements_history(credit.id, as_of(2026, 8, 18))?;
        assert_eq!(statements.len(), 1);
        assert_eq!(statements[0].statement_date, date(2026, 8, 10));
        assert_eq!(statements[0].amount, Decimal::from(300_u32));
        assert_eq!(statements[0].outstanding, Decimal::from(200_u32));
        assert_eq!(statements[0].due_at, Some(as_of(2026, 8, 25)));
        Ok(())
    }
}
