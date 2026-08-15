//! 借款：借出/借入、跨账户还款、未结余额与结清。

use chrono::Utc;
use rusqlite::{params, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{Loan, LoanType};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 借出/借入：创建借款记录并从账户划拨本金（借出扣减余额、借入增加余额）。
    pub fn create_loan(
        &mut self,
        loan_type: LoanType,
        counterparty: impl Into<String>,
        currency: impl Into<String>,
        amount: Decimal,
        account_id: i64,
        note: impl Into<String>,
    ) -> Result<Loan> {
        let counterparty = required_text(counterparty.into(), "counterparty")?;
        positive_amount(amount)?;
        let account = self.account(account_id)?;
        let currency = normalize_currency(currency.into())?;
        if currency != account.currency {
            return Err(KokuError::InvalidInput(
                "loan currency must match the account currency".to_owned(),
            ));
        }
        let now = Utc::now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // 同一往来人、同方向、同币种、未结清的借款合并：本金与未结余额累加。
        let loan_id = if let Some(existing_id) =
            Self::open_loan_id(&tx, loan_type, &counterparty, &currency)?
        {
            let (principal, outstanding) = Self::loan_totals(&tx, existing_id)?;
            tx.execute(
                "UPDATE loans SET principal = ?1, outstanding = ?2 WHERE id = ?3",
                params![
                    decimal_to_db(principal + amount),
                    decimal_to_db(outstanding + amount),
                    existing_id
                ],
            )?;
            existing_id
        } else {
            tx.execute(
                "INSERT INTO loans(loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    loan_type.as_str(),
                    &counterparty,
                    currency,
                    decimal_to_db(amount),
                    decimal_to_db(amount),
                    account_id,
                    timestamp(now),
                    note.into()
                ],
            )?;
            tx.last_insert_rowid()
        };
        let new_balance = match loan_type {
            LoanType::Lend => account.account_type.apply_outflow(account.balance, amount),
            LoanType::Borrow => account.account_type.apply_inflow(account.balance, amount),
        };
        Self::set_balance(&tx, account_id, new_balance)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, loan_id, occurred_at, note) VALUES ('loan', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                decimal_to_db(amount),
                currency,
                decimal_to_db(amount),
                loan_id,
                timestamp(now),
                format!("{} {counterparty}", loan_type.label())
            ],
        )?;
        tx.commit()?;
        self.loan(loan_id)
    }

    /// 查找同往来人/同方向/同币种且未结清的借款 id。
    fn open_loan_id(
        tx: &SqlTransaction<'_>,
        loan_type: LoanType,
        counterparty: &str,
        currency: &str,
    ) -> Result<Option<i64>> {
        tx.query_row(
            "SELECT id FROM loans WHERE loan_type = ?1 AND counterparty = ?2 AND currency = ?3 AND closed_at IS NULL ORDER BY id LIMIT 1",
            params![loan_type.as_str(), counterparty, currency],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(KokuError::from)
    }

    /// 读取借款的本金与未结余额（精确 Decimal 累加，避免 SQLite TEXT 转浮点丢精度）。
    fn loan_totals(tx: &SqlTransaction<'_>, id: i64) -> Result<(Decimal, Decimal)> {
        let (principal, outstanding) = tx.query_row(
            "SELECT principal, outstanding FROM loans WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        Ok((decimal_from_db(&principal)?, decimal_from_db(&outstanding)?))
    }

    /// 还款：资金从任意账户进出，递减借款未结余额，归零自动结清。
    pub fn repay_loan(
        &mut self,
        loan_id: i64,
        account_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        settled_amount: Option<Decimal>,
        note: impl Into<String>,
    ) -> Result<Loan> {
        positive_amount(amount)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let loan = Self::loan_in_tx(&tx, loan_id)?;
        if loan.closed_at.is_some() {
            return Err(KokuError::InvalidInput(
                "loan is already settled".to_owned(),
            ));
        }
        if amount > loan.outstanding {
            return Err(KokuError::InvalidInput(format!(
                "repayment {amount} exceeds the outstanding {}",
                loan.outstanding
            )));
        }
        let account = Self::account_in_tx(&tx, account_id)?;
        let currency = normalize_currency(currency.into())?;
        if currency != loan.currency {
            return Err(KokuError::InvalidInput(
                "repayment must be in the loan currency".to_owned(),
            ));
        }
        let settled = match settled_amount {
            Some(value) => value,
            None if currency == account.currency => amount,
            None => {
                return Err(KokuError::InvalidInput(format!(
                    "settled_amount in {} is required for a {currency} repayment",
                    account.currency
                )))
            }
        };
        let new_balance = match loan.loan_type {
            LoanType::Lend => account.account_type.apply_inflow(account.balance, settled),
            LoanType::Borrow => account.account_type.apply_outflow(account.balance, settled),
        };
        Self::set_balance(&tx, account_id, new_balance)?;
        let mut txn_note = format!("{}还款 {}", loan.loan_type.label(), loan.counterparty);
        let note = note.into();
        if !note.is_empty() {
            txn_note = format!("{txn_note} · {note}");
        }
        tx.execute(
            "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, loan_id, occurred_at, note) VALUES ('loan', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                decimal_to_db(amount),
                currency,
                decimal_to_db(settled),
                loan_id,
                timestamp(Utc::now()),
                txn_note
            ],
        )?;
        let outstanding = loan.outstanding - amount;
        let closed_at = if outstanding.is_zero() {
            Some(timestamp(Utc::now()))
        } else {
            None
        };
        tx.execute(
            "UPDATE loans SET outstanding = ?1, closed_at = ?2 WHERE id = ?3",
            params![decimal_to_db(outstanding), closed_at, loan_id],
        )?;
        tx.commit()?;
        self.loan(loan_id)
    }

    pub fn loans(&self) -> Result<Vec<Loan>> {
        let mut statement = self.conn.prepare(
            "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at FROM loans ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], loan_row)?;
        rows.map(|row| loan_from_row(row?)).collect()
    }

    pub fn loan(&self, id: i64) -> Result<Loan> {
        let row = self
            .conn
            .query_row(
                "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at FROM loans WHERE id = ?1",
                [id],
                loan_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound { entity: "loan", id })?;
        loan_from_row(row)
    }
}
