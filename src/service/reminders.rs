//! 到期提醒：汇总未来 N 天内到期（含已逾期）的定期存款、借款与信用卡账单。

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;

use super::*;
use crate::domain::LoanType;
use crate::error::Result;
use crate::service::BookkeepingService;

/// 一条到期提醒。
#[derive(Debug, Clone, Serialize)]
pub struct ReminderItem {
    /// "deposit" | "loan" | "credit_card"
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
            });
        }

        // 固定账单：账单是每月重复提醒，当前月份到期日已过时保留为逾期；否则提示本月即将到期。
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
                    });
                }
            }
        }

        items.sort_by_key(|item| item.due_at);
        Ok(items)
    }
}

fn bill_due_at(now: DateTime<Utc>, day: u32) -> Result<DateTime<Utc>> {
    let first = NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid current date".into()))?;
    let next_month = if now.month() == 12 {
        NaiveDate::from_ymd_opt(now.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1)
    }
    .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid current date".into()))?;
    let last_day = (next_month - Duration::days(1)).day();
    let date = first
        .with_day(day.min(last_day))
        .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid bill due day".into()))?;
    Ok(DateTime::from_naive_utc_and_offset(
        date.and_hms_opt(9, 0, 0)
            .ok_or_else(|| crate::error::KokuError::InvalidInput("invalid bill due time".into()))?,
        Utc,
    ))
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
        lines.push(format!(
            "- {} {} {}（{}，{}）",
            item.title,
            item.amount,
            item.currency,
            item.due_at.format("%Y-%m-%d"),
            when
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
}
