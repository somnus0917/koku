//! 到期提醒：汇总未来 N 天内到期（含已逾期）的定期存款、借款、信用卡与固定账单。

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Serialize;

use super::*;
use crate::domain::LoanType;
use crate::error::Result;
use crate::service::BookkeepingService;

/// 一条到期提醒。
#[derive(Debug, Clone, Serialize)]
pub struct ReminderItem {
    /// "deposit" | "loan" | "credit_card" | "bill" | "savings_goal"
    pub kind: String,
    pub id: i64,
    /// 展示标题：定存为备注（或占位文案），借款为往来方。
    pub title: String,
    pub amount: Decimal,
    pub currency: String,
    pub due_at: DateTime<Utc>,
    /// 是否已逾期。
    pub overdue: bool,
    /// 剩余天数（已逾期为负）。
    pub days_left: i64,
    /// 储蓄目标等进度型提醒的完成百分比。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_percent: Option<u32>,
}

impl BookkeepingService {
    /// 未来 `days` 天内到期（含已逾期）的定存与借款。
    pub fn due_reminders(&mut self, days: i64) -> Result<Vec<ReminderItem>> {
        let now = Utc::now();
        let horizon = now + Duration::days(days);
        let mut items = Vec::new();

        // 定期存款：未结清且到期日不晚于 horizon（含已逾期）。
        {
            let mut statement = self.conn.prepare(
                "SELECT id, amount, currency, maturity_at, note
                 FROM deposits
                 WHERE settled_at IS NULL AND maturity_at <= ?1
                 ORDER BY maturity_at",
            )?;
            let rows = statement.query_map([timestamp(horizon)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (id, amount, currency, maturity_at, note) = row?;
                let due_at = parse_timestamp(&maturity_at)?;
                items.push(ReminderItem {
                    kind: "deposit".to_owned(),
                    id,
                    title: if note.trim().is_empty() {
                        format!("定期存款 #{id}")
                    } else {
                        note.trim().to_owned()
                    },
                    amount: decimal_from_db(&amount)?,
                    currency,
                    overdue: due_at < now,
                    days_left: (due_at - now).num_days(),
                    due_at,
                    progress_percent: None,
                });
            }
        }

        // 借款：未结清、设定了到期日且不晚于 horizon。
        {
            let mut statement = self.conn.prepare(
                "SELECT id, loan_type, counterparty, currency, outstanding, due_at
                 FROM loans
                 WHERE closed_at IS NULL AND due_at IS NOT NULL AND due_at <= ?1
                 ORDER BY due_at",
            )?;
            let rows = statement.query_map([timestamp(horizon)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            for row in rows {
                let (id, loan_type, counterparty, currency, outstanding, due_at) = row?;
                let due_at = parse_timestamp(&due_at)?;
                let kind = LoanType::from_db(&loan_type)?.as_str();
                items.push(ReminderItem {
                    kind: "loan".to_owned(),
                    id,
                    title: match kind {
                        "lend" => format!("应收 {counterparty}"),
                        _ => format!("应付 {counterparty}"),
                    },
                    amount: decimal_from_db(&outstanding)?,
                    currency,
                    overdue: due_at < now,
                    days_left: (due_at - now).num_days(),
                    due_at,
                    progress_percent: None,
                });
            }
        }

        // 信用卡：只提示已出账快照中、按账户余额 FIFO 口径仍未还清的部分。
        for statement in self.due_credit_card_statements(now, horizon)? {
            let due_at = statement.due_at;
            items.push(ReminderItem {
                kind: "credit_card".to_owned(),
                id: statement.account_id,
                title: format!("信用卡 {}", statement.account_name),
                amount: statement.amount,
                currency: statement.currency,
                overdue: due_at < now,
                days_left: (due_at - now).num_days(),
                due_at,
                progress_percent: None,
            });
        }

        // 固定账单：选择本月尚未到达的到期日；本月日期已过则进位到下月。
        {
            let mut statement = self.conn.prepare(
                "SELECT b.id, b.name, b.amount, b.due_day, a.currency
                 FROM bills b JOIN accounts a ON a.id = b.account_id
                 WHERE b.active = 1 ORDER BY b.due_day, b.name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (id, name, amount, due_day, currency) = row?;
                let due_at = bill_due_at(now, due_day)?;
                if due_at <= horizon {
                    items.push(ReminderItem {
                        kind: "bill".to_owned(),
                        id,
                        title: name,
                        amount: decimal_from_db(&amount)?,
                        currency,
                        overdue: due_at < now,
                        days_left: (due_at - now).num_days(),
                        due_at,
                        progress_percent: None,
                    });
                }
            }
        }

        // 储蓄目标：仅提醒窗口内到期且尚未完成的目标。
        {
            let mut statement = self.conn.prepare(
                "SELECT g.id, g.name, g.target_amount, g.current_amount, g.target_date,
                        COALESCE(a.currency, 'CNY')
                 FROM savings_goals g
                 LEFT JOIN accounts a ON a.id = g.account_id
                 WHERE g.target_date IS NOT NULL AND g.target_date <= ?1
                 ORDER BY g.target_date, g.name",
            )?;
            let rows = statement.query_map([horizon.date_naive().to_string()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            for row in rows {
                let (id, name, target, current, target_date, currency) = row?;
                let target = decimal_from_db(&target)?;
                let current = decimal_from_db(&current)?;
                if current >= target {
                    continue;
                }
                let target_date =
                    NaiveDate::parse_from_str(&target_date, "%Y-%m-%d").map_err(|error| {
                        crate::error::KokuError::InvalidInput(format!(
                            "invalid savings goal target date: {error}"
                        ))
                    })?;
                let due_at = due_at_on(target_date)?;
                let progress_percent = ((current / target) * Decimal::from(100_u32))
                    .round_dp(0)
                    .to_u32()
                    .unwrap_or(0);
                items.push(ReminderItem {
                    kind: "savings_goal".to_owned(),
                    id,
                    title: name,
                    amount: target - current,
                    currency,
                    overdue: due_at < now,
                    days_left: (due_at - now).num_days(),
                    due_at,
                    progress_percent: Some(progress_percent),
                });
            }
        }

        items.sort_by_key(|item| item.due_at);
        Ok(items)
    }
}

fn bill_due_at(now: DateTime<Utc>, day: u32) -> Result<DateTime<Utc>> {
    let today = now.date_naive();
    let this_month = monthly_due_date(today.year(), today.month(), day)?;
    let date = if this_month < today {
        let (year, month) = if today.month() == 12 {
            (today.year() + 1, 1)
        } else {
            (today.year(), today.month() + 1)
        };
        monthly_due_date(year, month, day)?
    } else {
        this_month
    };
    due_at_on(date)
}

fn due_at_on(date: NaiveDate) -> Result<DateTime<Utc>> {
    Ok(DateTime::from_naive_utc_and_offset(
        date.and_hms_opt(9, 0, 0)
            .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid due time".into()))?,
        Utc,
    ))
}

fn monthly_due_date(year: i32, month: u32, day: u32) -> Result<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid bill month".into()))?;
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid bill month".into()))?;
    let last_day = (next_month - Duration::days(1)).day();
    first
        .with_day(day.min(last_day))
        .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid bill due day".into()))
}

/// 把到期提醒格式化为纯文本摘要（邮件正文用）。
pub fn reminder_digest_text(items: &[ReminderItem]) -> String {
    if items.is_empty() {
        return "当前没有到期提醒。".to_owned();
    }
    let mut lines = vec![format!("Koku 共 {} 项到期提醒：", items.len())];
    for item in items {
        let when = if item.overdue {
            format!("已逾期 {} 天", -item.days_left)
        } else {
            format!("{} 天后", item.days_left)
        };
        let progress = item
            .progress_percent
            .map(|percent| format!("，已完成 {percent}%"))
            .unwrap_or_default();
        lines.push(format!(
            "- {} {} {}（{}，{}{}）",
            item.title,
            item.amount,
            item.currency,
            item.due_at.format("%Y-%m-%d"),
            when,
            progress
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccountType, LoanType};

    fn test_service() -> Result<BookkeepingService> {
        BookkeepingService::in_memory()
    }

    #[test]
    fn lists_due_deposits_and_loans_within_horizon() -> Result<()> {
        let mut service = test_service()?;
        let account = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let now = Utc::now();

        // 5 天后到期的定存
        let due_soon = now + Duration::days(5);
        service.create_deposit(
            account.id,
            Decimal::from(1000_u32),
            "CNY",
            Decimal::from_str_exact("2.10").unwrap(),
            90,
            "三月定存",
        )?;
        // 手动把到期日改成 5 天后（create_deposit 按 term 计算到期日，这里直接 UPDATE）
        service.conn.execute(
            "UPDATE deposits SET maturity_at = ?1 WHERE note = '三月定存'",
            [timestamp(due_soon)],
        )?;

        // 40 天后到期的定存（超出 30 天窗口，不应出现）
        let far = now + Duration::days(40);
        service.create_deposit(
            account.id,
            Decimal::from(2000_u32),
            "CNY",
            Decimal::from_str_exact("2.10").unwrap(),
            90,
            "远期定存",
        )?;
        service.conn.execute(
            "UPDATE deposits SET maturity_at = ?1 WHERE note = '远期定存'",
            [timestamp(far)],
        )?;

        // 3 天后到期的借出
        let loan_due = now + Duration::days(3);
        service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(500_u32),
            account.id,
            "借给朋友",
            Some(loan_due),
        )?;

        let reminders = service.due_reminders(30)?;
        assert_eq!(reminders.len(), 2);
        // 排序：到期日升序 → 借款（3 天后）在前，定存（5 天后）在后
        assert_eq!(reminders[0].kind, "loan");
        assert_eq!(reminders[0].title, "应收 张三");
        assert!(!reminders[0].overdue);
        assert_eq!(reminders[1].kind, "deposit");
        assert_eq!(reminders[1].title, "三月定存");
        Ok(())
    }

    #[test]
    fn overdue_items_are_flagged() -> Result<()> {
        let mut service = test_service()?;
        let account = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let past = Utc::now() - Duration::days(2);
        service.create_deposit(
            account.id,
            Decimal::from(1000_u32),
            "CNY",
            Decimal::from_str_exact("2.10").unwrap(),
            90,
            "已过期定存",
        )?;
        service.conn.execute(
            "UPDATE deposits SET maturity_at = ?1 WHERE note = '已过期定存'",
            [timestamp(past)],
        )?;
        let reminders = service.due_reminders(30)?;
        assert_eq!(reminders.len(), 1);
        assert!(reminders[0].overdue);
        assert!(reminders[0].days_left < 0);
        Ok(())
    }

    #[test]
    fn includes_unpaid_credit_card_statement_snapshots() -> Result<()> {
        let mut service = test_service()?;
        let credit = service.create_account("信用卡", AccountType::Credit, "CNY", Decimal::ZERO)?;
        service.set_statement_day(credit.id, Some(10))?;
        let now = Utc::now();
        service.conn.execute(
            "UPDATE accounts SET balance = '200' WHERE id = ?1",
            [credit.id],
        )?;
        service.conn.execute(
            "INSERT INTO credit_card_statements (account_id, statement_date, due_at, amount, created_at)
             VALUES (?1, ?2, ?3, '200', ?4)",
            rusqlite::params![
                credit.id,
                now.date_naive().to_string(),
                timestamp(now + Duration::days(5)),
                timestamp(now),
            ],
        )?;

        let reminders = service.due_reminders(30)?;
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].kind, "credit_card");
        assert_eq!(reminders[0].title, "信用卡 信用卡");
        assert_eq!(reminders[0].amount, Decimal::from(200_u32));
        Ok(())
    }

    #[test]
    fn advances_recurring_bill_to_next_month_after_due_day() -> Result<()> {
        let now = DateTime::parse_from_rfc3339("2026-08-20T12:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            bill_due_at(now, 10)?.date_naive(),
            NaiveDate::from_ymd_opt(2026, 9, 10).unwrap()
        );
        Ok(())
    }

    #[test]
    fn clamps_recurring_bill_to_month_end() -> Result<()> {
        let now = DateTime::parse_from_rfc3339("2026-02-01T12:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            bill_due_at(now, 31)?.date_naive(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        );
        Ok(())
    }

    #[test]
    fn includes_only_active_bills_within_horizon() -> Result<()> {
        let mut service = test_service()?;
        let account = service.create_account(
            "日常账户",
            AccountType::Cash,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let category = service.create_category("住房", crate::domain::CategoryKind::Expense)?;
        let due_day = Utc::now().day();
        service.save_bill(
            None,
            "房租".into(),
            account.id,
            category.id,
            Decimal::from(3000_u32),
            due_day,
            true,
            String::new(),
        )?;
        service.save_bill(
            None,
            "停用订阅".into(),
            account.id,
            category.id,
            Decimal::from(30_u32),
            due_day,
            false,
            String::new(),
        )?;

        let reminders = service.due_reminders(31)?;
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].kind, "bill");
        assert_eq!(reminders[0].title, "房租");
        Ok(())
    }

    #[test]
    fn includes_only_incomplete_savings_goals_within_horizon() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("储蓄", AccountType::Savings, "CNY", Decimal::from(1000_u32))?;
        let target_date = Utc::now().date_naive() + Duration::days(5);
        service.save_savings_goal(
            None,
            "旅行基金".into(),
            Some(account.id),
            Decimal::from(1000_u32),
            Decimal::from(600_u32),
            Some(target_date),
        )?;
        service.save_savings_goal(
            None,
            "已完成目标".into(),
            Some(account.id),
            Decimal::from(1000_u32),
            Decimal::from(1000_u32),
            Some(target_date),
        )?;
        service.save_savings_goal(
            None,
            "远期目标".into(),
            Some(account.id),
            Decimal::from(1000_u32),
            Decimal::from(100_u32),
            Some(target_date + Duration::days(60)),
        )?;

        let reminders = service.due_reminders(30)?;
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].kind, "savings_goal");
        assert_eq!(reminders[0].title, "旅行基金");
        assert_eq!(reminders[0].amount, Decimal::from(400_u32));
        assert_eq!(reminders[0].progress_percent, Some(60));
        Ok(())
    }
}
