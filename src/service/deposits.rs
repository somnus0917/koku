//! 定期存款：独立实体（不再是一个账户），转入与到期结清。

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{AccountType, CategoryKind, Deposit, DepositSettlement};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 把储蓄账户中的一笔钱转为定期：原子扣款 + 建存款记录 + 记「转入定期」流水。
    #[allow(clippy::too_many_arguments)]
    pub fn create_deposit(
        &mut self,
        from_account_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        rate: Decimal,
        term_days: u32,
        note: impl Into<String>,
    ) -> Result<Deposit> {
        positive_amount(amount)?;
        if rate < Decimal::ZERO {
            return Err(KokuError::InvalidInput(
                "interest rate cannot be negative".to_owned(),
            ));
        }
        if term_days == 0 {
            return Err(KokuError::InvalidInput(
                "deposit term must be at least one day".to_owned(),
            ));
        }
        let source = self.account(from_account_id)?;
        if source.account_type != AccountType::Savings {
            return Err(KokuError::InvalidInput(
                "fixed deposits can only be opened from a savings account".to_owned(),
            ));
        }
        let currency = normalize_currency(currency.into())?;
        if currency != source.currency {
            return Err(KokuError::InvalidInput(
                "deposit currency must match the source account currency".to_owned(),
            ));
        }
        let now = Utc::now();
        let maturity = now + Duration::days(term_days as i64);
        let note = note.into();

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::set_balance(
            &tx,
            from_account_id,
            source.account_type.apply_outflow(source.balance, amount),
        )?;
        tx.execute(
            "INSERT INTO deposits(source_account_id, amount, currency, rate, term_days, opened_at, maturity_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                from_account_id,
                decimal_to_db(amount),
                &currency,
                decimal_to_db(rate),
                term_days,
                timestamp(now),
                timestamp(maturity),
                note
            ],
        )?;
        let deposit_id = tx.last_insert_rowid();
        insert_deposit_transaction(&tx, from_account_id, -amount, &currency, now, "转入定期")?;
        tx.commit()?;
        self.deposit(deposit_id)
    }

    pub fn deposit(&self, id: i64) -> Result<Deposit> {
        let row = self
            .conn
            .query_row(
                "SELECT id, source_account_id, amount, currency, rate, term_days, opened_at, maturity_at, settled_at, note
                 FROM deposits WHERE id = ?1",
                [id],
                deposit_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "deposit",
                id,
            })?;
        deposit_from_row(row)
    }

    pub fn deposits(&self) -> Result<Vec<Deposit>> {
        let mut statement = self.conn.prepare(
            "SELECT id, source_account_id, amount, currency, rate, term_days, opened_at, maturity_at, settled_at, note
             FROM deposits ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], deposit_row)?;
        rows.map(|row| deposit_from_row(row?)).collect()
    }

    /// 结清定期：按实际持有天数计息（记一笔利息收入到目标账户），
    /// 再把本金转回目标账户并标记结清。
    pub fn settle_deposit(
        &mut self,
        deposit_id: i64,
        to_account_id: i64,
    ) -> Result<DepositSettlement> {
        let deposit = self.deposit(deposit_id)?;
        if deposit.settled_at.is_some() {
            return Err(KokuError::InvalidInput(
                "deposit is already settled".to_owned(),
            ));
        }
        let target = self.account(to_account_id)?;
        if target.currency != deposit.currency {
            return Err(KokuError::InvalidInput(
                "settlement target must use the same currency as the deposit".to_owned(),
            ));
        }
        let now = Utc::now();
        let interest =
            calculate_simple_interest(deposit.amount, deposit.rate, deposit.opened_at, now);

        if interest > Decimal::ZERO {
            let interest_category = self.create_category("利息", CategoryKind::Income)?;
            self.record_income_in_currency(
                to_account_id,
                interest_category.id,
                interest,
                deposit.currency.clone(),
                interest,
                now,
                "定期利息",
            )?;
        }

        let principal = deposit.amount;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let target = Self::account_in_tx(&tx, to_account_id)?;
        Self::set_balance(
            &tx,
            to_account_id,
            target.account_type.apply_inflow(target.balance, principal),
        )?;
        insert_deposit_transaction(
            &tx,
            to_account_id,
            principal,
            &deposit.currency,
            now,
            "定期到期转回",
        )?;
        let transfer_id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE deposits SET settled_at = ?1 WHERE id = ?2",
            params![timestamp(now), deposit_id],
        )?;
        tx.commit()?;
        let transfer = self.transaction(transfer_id)?;
        Ok(DepositSettlement { interest, transfer })
    }
}

fn insert_deposit_transaction(
    tx: &rusqlite::Transaction<'_>,
    account_id: i64,
    signed_amount: Decimal,
    currency: &str,
    occurred_at: DateTime<Utc>,
    note: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, occurred_at, note)
         VALUES ('deposit', ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account_id,
            decimal_to_db(signed_amount),
            currency,
            decimal_to_db(signed_amount),
            timestamp(occurred_at),
            note
        ],
    )?;
    Ok(())
}

type DepositRow = (
    i64,
    i64,
    String,
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    String,
);

fn deposit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DepositRow> {
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

fn deposit_from_row(row: DepositRow) -> Result<Deposit> {
    Ok(Deposit {
        id: row.0,
        source_account_id: row.1,
        amount: decimal_from_db(&row.2)?,
        currency: row.3,
        rate: decimal_from_db(&row.4)?,
        term_days: row.5 as u32,
        opened_at: parse_timestamp(&row.6)?,
        maturity_at: parse_timestamp(&row.7)?,
        settled_at: row.8.as_deref().map(parse_timestamp).transpose()?,
        note: row.9,
    })
}
