//! 周期交易：保存模板并在请求时把到期的规则落库为真实流水。

use chrono::{DateTime, Duration, Months, Utc};
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{
    CategoryKind, RecurrenceFrequency, RecurringOccurrence, RecurringRule, Transaction,
    TransactionKind,
};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

/// 单轮最多为每条规则追赶多少期，避免停服很久后一次性生成海量流水。
const MAX_CATCHUP_PER_RULE: u32 = 12;

impl BookkeepingService {
    #[allow(clippy::too_many_arguments)]
    pub fn create_recurring_rule(
        &mut self,
        kind: TransactionKind,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        note: String,
        frequency: RecurrenceFrequency,
        next_due_at: DateTime<Utc>,
    ) -> Result<RecurringRule> {
        positive_amount(amount)?;
        if !matches!(kind, TransactionKind::Expense | TransactionKind::Income) {
            return Err(KokuError::InvalidInput(
                "recurring rules must be expense or income".to_owned(),
            ));
        }
        let category = self.category(category_id)?;
        let expected = match kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            _ => unreachable!("validated above"),
        };
        if category.kind != expected {
            return Err(KokuError::CategoryKindMismatch {
                expected: expected.as_str(),
                actual: category.kind.as_str(),
            });
        }
        self.conn.execute(
            "INSERT INTO recurring_rules(kind, account_id, category_id, amount, note, frequency, next_due_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                kind.as_str(),
                account_id,
                category_id,
                decimal_to_db(amount),
                note,
                frequency.as_str(),
                timestamp(next_due_at),
                timestamp(Utc::now())
            ],
        )?;
        self.recurring_rule(self.conn.last_insert_rowid())
    }

    pub fn recurring_rule(&self, id: i64) -> Result<RecurringRule> {
        let row = self
            .conn
            .query_row(
                "SELECT id, kind, account_id, category_id, amount, note, frequency, next_due_at, paused_at
                 FROM recurring_rules WHERE id = ?1",
                [id],
                recurring_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "recurring rule",
                id,
            })?;
        recurring_from_row(row)
    }

    pub fn recurring_rules(&self) -> Result<Vec<RecurringRule>> {
        let mut statement = self.conn.prepare(
            "SELECT id, kind, account_id, category_id, amount, note, frequency, next_due_at, paused_at
             FROM recurring_rules ORDER BY next_due_at, id",
        )?;
        let rows = statement.query_map([], recurring_row)?;
        rows.map(|row| recurring_from_row(row?)).collect()
    }

    pub fn delete_recurring_rule(&mut self, id: i64) -> Result<RecurringRule> {
        let rule = self.recurring_rule(id)?;
        self.conn
            .execute("DELETE FROM recurring_rules WHERE id = ?1", [id])?;
        Ok(rule)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_recurring_rule(
        &mut self,
        id: i64,
        kind: TransactionKind,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        note: String,
        frequency: RecurrenceFrequency,
        next_due_at: DateTime<Utc>,
    ) -> Result<RecurringRule> {
        positive_amount(amount)?;
        if !matches!(kind, TransactionKind::Expense | TransactionKind::Income) {
            return Err(KokuError::InvalidInput(
                "recurring rules must be expense or income".to_owned(),
            ));
        }
        self.account(account_id)?;
        let category = self.category(category_id)?;
        let expected = if kind == TransactionKind::Expense {
            CategoryKind::Expense
        } else {
            CategoryKind::Income
        };
        if category.kind != expected {
            return Err(KokuError::CategoryKindMismatch {
                expected: expected.as_str(),
                actual: category.kind.as_str(),
            });
        }
        self.conn.execute(
            "UPDATE recurring_rules SET kind = ?1, account_id = ?2, category_id = ?3, amount = ?4, note = ?5, frequency = ?6, next_due_at = ?7 WHERE id = ?8",
            params![kind.as_str(), account_id, category_id, decimal_to_db(amount), note, frequency.as_str(), timestamp(next_due_at), id],
        )?;
        self.recurring_rule(id)
    }

    pub fn set_recurring_paused(&mut self, id: i64, paused: bool) -> Result<RecurringRule> {
        self.recurring_rule(id)?;
        self.conn.execute(
            "UPDATE recurring_rules SET paused_at = ?1 WHERE id = ?2",
            params![
                if paused {
                    Some(timestamp(Utc::now()))
                } else {
                    None
                },
                id
            ],
        )?;
        self.recurring_rule(id)
    }

    pub fn recurring_preview(&self, id: i64, count: usize) -> Result<Vec<RecurringOccurrence>> {
        let rule = self.recurring_rule(id)?;
        let mut due_at = rule.next_due_at;
        let mut values = Vec::with_capacity(count.min(12));
        for _ in 0..count.min(12) {
            values.push(RecurringOccurrence { due_at });
            due_at = advance_next_due(due_at, rule.frequency);
        }
        Ok(values)
    }

    /// 把到期未生成的周期规则落库为真实流水，并推进 `next_due_at`。
    /// 某条规则生成失败时跳过该规则（保留 next_due_at 下次重试），不影响其余规则。
    pub fn run_recurring(&mut self) -> Result<Vec<Transaction>> {
        let now = Utc::now();
        let due = self.due_recurring_rules(now)?;
        let mut generated = Vec::new();
        for rule in due {
            let mut next = rule.next_due_at;
            let mut produced = 0_u32;
            while next <= now && produced < MAX_CATCHUP_PER_RULE {
                let result = match rule.kind {
                    TransactionKind::Expense => self.record_expense(
                        rule.account_id,
                        rule.category_id,
                        rule.amount,
                        next,
                        rule.note.clone(),
                    ),
                    TransactionKind::Income => self.record_income(
                        rule.account_id,
                        rule.category_id,
                        rule.amount,
                        next,
                        rule.note.clone(),
                    ),
                    _ => unreachable!("validated at creation"),
                };
                match result {
                    Ok(transaction) => {
                        generated.push(transaction);
                        produced += 1;
                        next = advance_next_due(next, rule.frequency);
                    }
                    Err(error) => {
                        tracing::warn!(target: "recurring", rule_id = rule.id, error = %error, "recurring rule occurrence failed; skipping rule");
                        break;
                    }
                }
            }
            if produced > 0 {
                self.conn.execute(
                    "UPDATE recurring_rules SET next_due_at = ?1 WHERE id = ?2",
                    params![timestamp(next), rule.id],
                )?;
            }
        }
        Ok(generated)
    }

    fn due_recurring_rules(&self, now: DateTime<Utc>) -> Result<Vec<RecurringRule>> {
        let mut statement = self.conn.prepare(
            "SELECT id, kind, account_id, category_id, amount, note, frequency, next_due_at, paused_at
             FROM recurring_rules
             WHERE paused_at IS NULL AND next_due_at <= ?1
             ORDER BY next_due_at, id",
        )?;
        let rows = statement.query_map([timestamp(now)], recurring_row)?;
        rows.map(|row| recurring_from_row(row?)).collect()
    }
}

fn advance_next_due(current: DateTime<Utc>, frequency: RecurrenceFrequency) -> DateTime<Utc> {
    match frequency {
        RecurrenceFrequency::Weekly => current + Duration::days(7),
        RecurrenceFrequency::Monthly => current
            .checked_add_months(Months::new(1))
            .unwrap_or_else(|| current + Duration::days(30)),
    }
}

type RecurringRow = (
    i64,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
);

fn recurring_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecurringRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn recurring_from_row(row: RecurringRow) -> Result<RecurringRule> {
    Ok(RecurringRule {
        id: row.0,
        kind: TransactionKind::from_db(&row.1)?,
        account_id: row.2,
        category_id: row.3,
        amount: decimal_from_db(&row.4)?,
        note: row.5,
        frequency: RecurrenceFrequency::from_db(&row.6)?,
        next_due_at: parse_timestamp(&row.7)?,
        paused_at: row.8.as_deref().map(parse_timestamp).transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;

    #[test]
    fn recurring_rules_can_be_edited_paused_and_previewed() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let account = service.create_account("现金", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let category = service
            .categories()?
            .into_iter()
            .find(|category| category.name == "餐饮")
            .expect("default food category");
        let first_due = Utc::now() + Duration::days(3);
        let rule = service.create_recurring_rule(
            TransactionKind::Expense,
            account.id,
            category.id,
            Decimal::from(20_u32),
            "午餐".to_owned(),
            RecurrenceFrequency::Weekly,
            first_due,
        )?;
        let updated = service.update_recurring_rule(
            rule.id,
            TransactionKind::Expense,
            account.id,
            category.id,
            Decimal::from(25_u32),
            "订阅".to_owned(),
            RecurrenceFrequency::Monthly,
            first_due,
        )?;
        assert_eq!(updated.note, "订阅");
        assert_eq!(updated.amount, Decimal::from(25_u32));
        assert_eq!(updated.frequency, RecurrenceFrequency::Monthly);

        let paused = service.set_recurring_paused(rule.id, true)?;
        assert!(paused.paused_at.is_some());
        let preview = service.recurring_preview(rule.id, 3)?;
        assert_eq!(preview.len(), 3);
        assert_eq!(preview[0].due_at.timestamp(), first_due.timestamp());
        assert!(preview[1].due_at > preview[0].due_at);
        assert!(service
            .set_recurring_paused(rule.id, false)?
            .paused_at
            .is_none());
        Ok(())
    }
}
