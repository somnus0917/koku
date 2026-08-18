//! 信用卡账单模型 v1：额度占用、账单周期与出账/未出账金额。
//!
//! # 核心语义
//! - 消费 = Credit 账户上的 Expense（正常支出）；
//! - 还款 = 储蓄账户 → Credit 账户的 Transfer（**不是** Expense，避免重复统计）；
//! - 余额与额度只算一次：账户余额继续由现有 `AccountType` 语义维护
//!   （Credit 为负债：支出使余额增加、还款使余额减少）。
//!
//! # 账单口径（v1，可解释近似，无快照表，按现有交易动态计算）
//! - 消费：该账户上未撤销的 Expense，按 `settled_amount`（账户币种结算额）计；
//! - 还款：转入该账户的未撤销 Transfer，按 `target_amount`（账户币种到账额）计；
//! - 还款按 FIFO 冲抵**最早发生**的消费（先旧后新），
//!   `used_credit = max(0, 全部消费 − 全部还款)`；
//! - `current_statement_amount` = 最近一期已出账周期（[上一账单日, 最近账单日)）
//!   中未被还款冲抵的消费；
//! - `unbilled_amount` = 最近账单日之后、截至 as_of 的未出账消费
//!   （还款冲抵完所有已出账消费后才开始冲抵未出账部分）。
//!
//! 本版不追踪「某次还款具体偿还哪一期」，采用上述 FIFO/总负债近似并在
//! README 中说明；不做最低还款/分期/利息/罚息等结算引擎。
//!
//! 所有日期 helper 显式接收日期参数，不内嵌 `Utc::now()`，保证可测试。

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use rusqlite::{params_from_iter, types::Value};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;

use super::*;
use crate::domain::{AccountType, CreditCardSummary};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 计算信用卡账单摘要（`as_of` 为快照时间点；日期 helper 均显式传参）。
    ///
    /// 仅对 Credit 账户有效；非 Credit 账户返回明确错误。`statement_day` /
    /// `due_day` 未设置时返回部分字段为 `None` 的部分摘要（不 panic）。
    pub fn credit_card_summary(
        &self,
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
        let (current_statement_amount, unbilled_amount, used_credit) =
            self.credit_card_amounts(account_id, as_of, account.statement_day)?;
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

    /// 计算额度占用与出账/未出账金额（summary 与还款提醒共用）。
    ///
    /// 返回 `(current_statement_amount, unbilled_amount, used_credit)`；
    /// `statement_day` 为 `None` 时前两项为 `None`（无账单周期）。
    pub(super) fn credit_card_amounts(
        &self,
        account_id: i64,
        as_of: DateTime<Utc>,
        statement_day: Option<u32>,
    ) -> Result<(Option<Decimal>, Option<Decimal>, Decimal)> {
        let Some(statement_day) = statement_day else {
            let expenses = self.sum_expenses(account_id, &as_of, None, None)?;
            let repayments = self.sum_repayments(account_id, &as_of)?;
            let used = (expenses - repayments).max(Decimal::ZERO);
            return Ok((None, None, used));
        };
        let as_of_date = as_of.date_naive();
        let recent = recent_statement_date(as_of_date, statement_day);
        let prev = previous_statement_date(recent, statement_day);
        let prev_midnight = midnight_utc(prev);
        let recent_midnight = midnight_utc(recent);
        let before_prev = self.sum_expenses(account_id, &as_of, None, Some(&prev_midnight))?;
        let current = self.sum_expenses(
            account_id,
            &as_of,
            Some(&prev_midnight),
            Some(&recent_midnight),
        )?;
        let unbilled = self.sum_expenses(account_id, &as_of, Some(&recent_midnight), None)?;
        let repayments = self.sum_repayments(account_id, &as_of)?;
        // FIFO：还款先冲抵最早周期的消费。
        let mut remaining = repayments;
        let before_prev_unpaid = (before_prev - remaining).max(Decimal::ZERO);
        remaining = (remaining - before_prev).max(Decimal::ZERO);
        let current_unpaid = (current - remaining).max(Decimal::ZERO);
        remaining = (remaining - current).max(Decimal::ZERO);
        let unbilled_unpaid = (unbilled - remaining).max(Decimal::ZERO);
        let used = before_prev_unpaid + current_unpaid + unbilled_unpaid;
        Ok((Some(current_unpaid), Some(unbilled_unpaid), used))
    }

    /// 账户上未撤销 Expense 的 `settled_amount` 之和（可加下/上界过滤）。
    fn sum_expenses(
        &self,
        account_id: i64,
        as_of: &DateTime<Utc>,
        lower: Option<&DateTime<Utc>>,
        upper: Option<&DateTime<Utc>>,
    ) -> Result<Decimal> {
        self.sum_amount(
            "settled_amount",
            "account_id = ? AND kind = 'expense' AND voided_at IS NULL AND occurred_at <= ?",
            account_id,
            as_of,
            lower,
            upper,
        )
    }

    /// 转入账户的未撤销 Transfer 的 `target_amount` 之和（还款口径）。
    fn sum_repayments(&self, account_id: i64, as_of: &DateTime<Utc>) -> Result<Decimal> {
        self.sum_amount(
            "target_amount",
            "to_account_id = ? AND kind = 'transfer' AND voided_at IS NULL AND occurred_at <= ?",
            account_id,
            as_of,
            None,
            None,
        )
    }

    /// 通用金额求和：按账户/时间过滤，SQL 内先求 REAL 和，回 Rust 转 Decimal 并取两位小数
    /// （与既有统计口径一致；金额最终均为 Decimal）。
    fn sum_amount(
        &self,
        column: &str,
        base_where: &str,
        account_id: i64,
        as_of: &DateTime<Utc>,
        lower: Option<&DateTime<Utc>>,
        upper: Option<&DateTime<Utc>>,
    ) -> Result<Decimal> {
        let mut sql = format!(
            "SELECT COALESCE(SUM(CAST({column} AS REAL)), 0) FROM transactions WHERE {base_where}"
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
        let value: f64 = self
            .conn
            .query_row(&sql, params_from_iter(params), |row| row.get(0))?;
        Ok(Decimal::from_f64(value).unwrap_or_default().round_dp(2))
    }
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
        let (service, _, cash, _) = seeded()?;
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
}
