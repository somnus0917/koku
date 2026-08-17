//! 账户对账工作流：以对账单余额为目标余额，核对账面余额，
//! 完成时自动生成 adjustment 流水补齐差额（可撤销、可审计）。

use chrono::{NaiveDate, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{Reconciliation, ReconciliationStatus};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 创建一笔对账：同一账户同一时间只能有一笔进行中的对账。
    pub fn create_reconciliation(
        &mut self,
        account_id: i64,
        statement_date: &str,
        statement_balance: Decimal,
        note: &str,
    ) -> Result<Reconciliation> {
        // 校验对账日为合法自然日（YYYY-MM-DD）。
        NaiveDate::parse_from_str(statement_date, "%Y-%m-%d").map_err(|error| {
            KokuError::InvalidInput(format!("statement_date must be YYYY-MM-DD: {error}"))
        })?;
        let account = self.account(account_id)?;
        let open_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM reconciliations
             WHERE account_id = ?1 AND status = 'open'",
            [account_id],
            |row| row.get(0),
        )?;
        if open_count > 0 {
            return Err(KokuError::InvalidInput(
                "该账户已有一笔进行中的对账，请先完成或取消".to_owned(),
            ));
        }
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO reconciliations(account_id, statement_date, statement_balance, book_balance, status, opened_at, note)
             VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6)",
            params![
                account_id,
                statement_date,
                decimal_to_db(statement_balance),
                decimal_to_db(account.balance),
                timestamp(now),
                note
            ],
        )?;
        self.reconciliation(self.conn.last_insert_rowid())
    }

    pub fn reconciliation(&self, id: i64) -> Result<Reconciliation> {
        self.conn
            .query_row(
                "SELECT id, account_id, statement_date, statement_balance, book_balance,
                        status, opened_at, completed_at, adjustment_transaction_id, note
                 FROM reconciliations WHERE id = ?1",
                [id],
                reconciliation_row,
            )
            .optional()?
            .map(reconciliation_from_row)
            .transpose()?
            .ok_or(KokuError::NotFound {
                entity: "reconciliation",
                id,
            })
    }

    /// 列出对账记录；`account_id` 给定时只列该账户，缺省列全部。
    pub fn reconciliations(&self, account_id: Option<i64>) -> Result<Vec<Reconciliation>> {
        let mut statement = match account_id {
            Some(account_id) => {
                let mut statement = self.conn.prepare(
                    "SELECT id, account_id, statement_date, statement_balance, book_balance,
                            status, opened_at, completed_at, adjustment_transaction_id, note
                     FROM reconciliations WHERE account_id = ?1 ORDER BY id DESC",
                )?;
                let rows = statement.query_map([account_id], reconciliation_row)?;
                let mut result = Vec::new();
                for row in rows {
                    result.push(reconciliation_from_row(row?)?);
                }
                return Ok(result);
            }
            None => self.conn.prepare(
                "SELECT id, account_id, statement_date, statement_balance, book_balance,
                        status, opened_at, completed_at, adjustment_transaction_id, note
                 FROM reconciliations ORDER BY id DESC",
            )?,
        };
        let rows = statement.query_map([], reconciliation_row)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(reconciliation_from_row(row?)?);
        }
        Ok(result)
    }

    /// 完成对账：差额 = 对账单余额 − 当前账面余额；差额非零时自动生成
    /// adjustment 流水（note 带对账编号，可撤销）。返回更新后的对账记录。
    pub fn complete_reconciliation(&mut self, id: i64) -> Result<Reconciliation> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = {
            let row = tx
                .query_row(
                    "SELECT id, account_id, statement_date, statement_balance, book_balance,
                            status, opened_at, completed_at, adjustment_transaction_id, note
                     FROM reconciliations WHERE id = ?1",
                    [id],
                    reconciliation_row,
                )
                .optional()?;
            row.map(reconciliation_from_row)
                .transpose()?
                .ok_or(KokuError::NotFound {
                    entity: "reconciliation",
                    id,
                })?
        };
        if current.status != ReconciliationStatus::Open {
            return Err(KokuError::InvalidInput(
                "只有进行中的对账才能完成".to_owned(),
            ));
        }
        let account = Self::account_in_tx(&tx, current.account_id)?;
        let difference = current.statement_balance - account.balance;

        let adjustment_transaction_id = if difference.is_zero() {
            None
        } else {
            // 生成带对账编号的调整流水（balance += difference）。
            let new_balance = account.balance + difference;
            Self::set_balance(&tx, current.account_id, new_balance)?;
            let note = format!(
                "对账调整 #{}（{}{}）",
                current.id,
                current.statement_date,
                if current.note.trim().is_empty() {
                    String::new()
                } else {
                    format!("，{}", current.note.trim())
                }
            );
            tx.execute(
                "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, occurred_at, note)
                 VALUES ('adjustment', ?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    current.account_id,
                    decimal_to_db(difference),
                    account.currency,
                    decimal_to_db(difference),
                    timestamp(Utc::now()),
                    note
                ],
            )?;
            Some(tx.last_insert_rowid())
        };

        tx.execute(
            "UPDATE reconciliations
             SET status = 'completed', completed_at = ?1, adjustment_transaction_id = ?2
             WHERE id = ?3",
            params![timestamp(Utc::now()), adjustment_transaction_id, id],
        )?;
        tx.commit()?;
        self.reconciliation(id)
    }

    /// 取消对账：不产生任何调整。
    pub fn cancel_reconciliation(&mut self, id: i64) -> Result<Reconciliation> {
        let current = self.reconciliation(id)?;
        if current.status != ReconciliationStatus::Open {
            return Err(KokuError::InvalidInput(
                "只有进行中的对账才能取消".to_owned(),
            ));
        }
        self.conn.execute(
            "UPDATE reconciliations SET status = 'cancelled', completed_at = ?1 WHERE id = ?2",
            params![timestamp(Utc::now()), id],
        )?;
        self.reconciliation(id)
    }
}

fn reconciliation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReconciliationRow> {
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
        row.get(9)?,
    ))
}

type ReconciliationRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    String,
);

fn reconciliation_from_row(row: ReconciliationRow) -> Result<Reconciliation> {
    Ok(Reconciliation {
        id: row.0,
        account_id: row.1,
        statement_date: row.2,
        statement_balance: decimal_from_db(&row.3)?,
        book_balance: decimal_from_db(&row.4)?,
        status: ReconciliationStatus::from_db(&row.5)?,
        opened_at: parse_timestamp(&row.6)?,
        completed_at: row.7.as_deref().map(parse_timestamp).transpose()?,
        adjustment_transaction_id: row.8,
        note: row.9,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccountType, TransactionKind};

    fn test_service() -> Result<BookkeepingService> {
        BookkeepingService::in_memory()
    }

    #[test]
    fn complete_creates_adjustment_for_difference() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        // 对账单余额 1234.56，账面 1000 → 差额 +234.56
        let reconciliation = service.create_reconciliation(
            account.id,
            "2026-03-31",
            Decimal::from_str_exact("1234.56").unwrap(),
            "三月对账",
        )?;
        assert_eq!(reconciliation.status, ReconciliationStatus::Open);
        assert_eq!(reconciliation.book_balance, Decimal::from(1000_u32));

        let completed = service.complete_reconciliation(reconciliation.id)?;
        assert_eq!(completed.status, ReconciliationStatus::Completed);
        assert!(completed.adjustment_transaction_id.is_some());
        let account = service.account(account.id)?;
        assert_eq!(account.balance, Decimal::from_str_exact("1234.56").unwrap());
        // 调整流水可审计：kind=adjustment，note 带对账编号
        let adjustment = service.transaction(completed.adjustment_transaction_id.unwrap())?;
        assert_eq!(adjustment.kind, TransactionKind::Adjustment);
        assert!(adjustment.note.contains("#1"));
        assert!(adjustment.note.contains("三月对账"));
        Ok(())
    }

    #[test]
    fn complete_with_zero_difference_skips_adjustment() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(500_u32))?;
        let reconciliation =
            service.create_reconciliation(account.id, "2026-03-31", Decimal::from(500_u32), "")?;
        let completed = service.complete_reconciliation(reconciliation.id)?;
        assert_eq!(completed.status, ReconciliationStatus::Completed);
        assert_eq!(completed.adjustment_transaction_id, None);
        assert_eq!(service.account(account.id)?.balance, Decimal::from(500_u32));
        Ok(())
    }

    #[test]
    fn one_open_reconciliation_per_account() -> Result<()> {
        let mut service = test_service()?;
        let account = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        service.create_reconciliation(account.id, "2026-03-31", Decimal::ZERO, "")?;
        assert!(service
            .create_reconciliation(account.id, "2026-04-30", Decimal::ZERO, "")
            .is_err());
        // 完成后可以再开新对账
        let open = service
            .reconciliations(Some(account.id))?
            .into_iter()
            .find(|item| item.status == ReconciliationStatus::Open)
            .unwrap();
        service.complete_reconciliation(open.id)?;
        service.create_reconciliation(account.id, "2026-04-30", Decimal::ZERO, "")?;
        Ok(())
    }

    #[test]
    fn cancel_leaves_balance_untouched() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(300_u32))?;
        let reconciliation =
            service.create_reconciliation(account.id, "2026-03-31", Decimal::from(999_u32), "")?;
        let cancelled = service.cancel_reconciliation(reconciliation.id)?;
        assert_eq!(cancelled.status, ReconciliationStatus::Cancelled);
        assert_eq!(service.account(account.id)?.balance, Decimal::from(300_u32));
        // 已完成/已取消的对账不能再完成或取消
        assert!(service.complete_reconciliation(cancelled.id).is_err());
        assert!(service.cancel_reconciliation(cancelled.id).is_err());
        Ok(())
    }
}
