//! 报销：标记/取消待报销、部分报销（生成关联收入流水）。

use chrono::Utc;
use rusqlite::{params, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{CategoryKind, Transaction, TransactionKind};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 给一笔支出打上"待报销"标记。
    pub fn mark_reimbursable(&mut self, transaction_id: i64) -> Result<Transaction> {
        let transaction = self.transaction(transaction_id)?;
        if transaction.kind != TransactionKind::Expense {
            return Err(KokuError::InvalidInput(
                "only expenses can be marked as reimbursable".to_owned(),
            ));
        }
        if transaction.voided_at.is_some() {
            return Err(KokuError::InvalidInput(
                "voided transactions cannot be marked as reimbursable".to_owned(),
            ));
        }
        if transaction.reimbursed_at.is_some() {
            return Err(KokuError::InvalidInput(
                "fully reimbursed transactions cannot be marked again".to_owned(),
            ));
        }
        if transaction.reimbursable_at.is_none() {
            self.conn.execute(
                "UPDATE transactions SET reimbursable_at = ?1 WHERE id = ?2 AND reimbursable_at IS NULL",
                params![timestamp(Utc::now()), transaction_id],
            )?;
        }
        self.transaction(transaction_id)
    }

    /// 取消"待报销"标记；已发生报销的支出不允许取消。
    pub fn unmark_reimbursable(&mut self, transaction_id: i64) -> Result<Transaction> {
        let transaction = self.transaction(transaction_id)?;
        if transaction.reimbursable_at.is_none() {
            return Err(KokuError::InvalidInput(
                "transaction is not marked as reimbursable".to_owned(),
            ));
        }
        if transaction.reimbursed_at.is_some() || !transaction.reimbursed_amount.is_zero() {
            return Err(KokuError::InvalidInput(
                "cannot unmark a transaction that already has reimbursements".to_owned(),
            ));
        }
        self.conn.execute(
            "UPDATE transactions SET reimbursable_at = NULL WHERE id = ?1",
            [transaction_id],
        )?;
        self.transaction(transaction_id)
    }

    /// 报销一笔待报销支出：生成关联的收入流水（可入任意账户），支持部分报销。
    pub fn reimburse(
        &mut self,
        expense_id: i64,
        account_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        settled_amount: Option<Decimal>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        positive_amount(amount)?;
        let expense = self.transaction(expense_id)?;
        if expense.kind != TransactionKind::Expense {
            return Err(KokuError::InvalidInput(
                "reimbursements can only settle expense transactions".to_owned(),
            ));
        }
        if expense.voided_at.is_some() {
            return Err(KokuError::InvalidInput(
                "voided transactions cannot be reimbursed".to_owned(),
            ));
        }
        if expense.reimbursable_at.is_none() {
            return Err(KokuError::InvalidInput(
                "transaction is not marked as reimbursable".to_owned(),
            ));
        }
        if expense.reimbursed_at.is_some() {
            return Err(KokuError::InvalidInput(
                "transaction is already fully reimbursed".to_owned(),
            ));
        }
        let currency = normalize_currency(currency.into())?;
        if currency != expense.currency {
            return Err(KokuError::InvalidInput(format!(
                "reimbursement must be in the expense currency {}",
                expense.currency
            )));
        }
        let remaining = expense.amount - expense.reimbursed_amount - expense.refunded_amount;
        if amount > remaining {
            return Err(KokuError::InvalidInput(format!(
                "reimbursement amount {amount} exceeds the remaining {remaining}"
            )));
        }

        let reimburse_category = self.create_category("报销", CategoryKind::Income)?;
        let now = Utc::now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        let category = Self::category_in_tx(&tx, reimburse_category.id)?;
        let settled = match settled_amount {
            Some(value) => value,
            None if currency == account.currency => amount,
            None => {
                return Err(KokuError::InvalidInput(format!(
                    "settled_amount in {} is required for a {currency} reimbursement",
                    account.currency
                )))
            }
        };
        if currency == account.currency && settled != amount {
            return Err(KokuError::InvalidInput(
                "same-currency reimbursements must settle for the original amount".to_owned(),
            ));
        }
        let new_balance = account.account_type.apply_inflow(account.balance, settled);
        Self::set_balance(&tx, account_id, new_balance)?;
        // 报销收入按被报销支出的日期入账：支出与其报销落在同一个月，
        // 避免跨月报销时历史月份的报表被追溯改写、单月收支不自洽。
        tx.execute(
            "INSERT INTO transactions(kind, account_id, category_id, amount, currency, settled_amount, occurred_at, note) VALUES ('income', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                category.id,
                decimal_to_db(amount),
                currency,
                decimal_to_db(settled),
                timestamp(expense.occurred_at),
                note.into()
            ],
        )?;
        let income_id = tx.last_insert_rowid();
        let new_reimbursed = expense.reimbursed_amount + amount;
        let fully_reimbursed = new_reimbursed >= expense.amount;
        let reimbursed_at = if fully_reimbursed {
            Some(timestamp(now))
        } else {
            None
        };
        tx.execute(
            "UPDATE transactions SET reimbursed_amount = ?1, reimbursed_at = ?2 WHERE id = ?3",
            params![decimal_to_db(new_reimbursed), reimbursed_at, expense_id],
        )?;
        tx.execute(
            "INSERT INTO reimbursements(expense_id, income_id, amount, reimbursed_at) VALUES (?1, ?2, ?3, ?4)",
            params![expense_id, income_id, decimal_to_db(amount), timestamp(now)],
        )?;
        tx.commit()?;
        self.transaction(income_id)
    }

    /// 为一笔支出登记商户退款：生成关联收入，可入任意指定账户并支持部分退款。
    #[allow(clippy::too_many_arguments)]
    pub fn refund(
        &mut self,
        expense_id: i64,
        account_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        settled_amount: Option<Decimal>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        positive_amount(amount)?;
        let expense = self.transaction(expense_id)?;
        if expense.kind != TransactionKind::Expense {
            return Err(KokuError::InvalidInput(
                "refunds can only be created for expense transactions".to_owned(),
            ));
        }
        if expense.voided_at.is_some() {
            return Err(KokuError::InvalidInput(
                "voided transactions cannot be refunded".to_owned(),
            ));
        }
        let currency = normalize_currency(currency.into())?;
        if currency != expense.currency {
            return Err(KokuError::InvalidInput(format!(
                "refund must be in the expense currency {}",
                expense.currency
            )));
        }
        let remaining = expense.amount - expense.reimbursed_amount - expense.refunded_amount;
        if amount > remaining {
            return Err(KokuError::InvalidInput(format!(
                "refund amount {amount} exceeds the remaining {remaining}"
            )));
        }

        let refund_category = self.create_category("退款", CategoryKind::Income)?;
        let now = Utc::now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        let category = Self::category_in_tx(&tx, refund_category.id)?;
        let settled = match settled_amount {
            Some(value) => value,
            None if currency == account.currency => amount,
            None => {
                return Err(KokuError::InvalidInput(format!(
                    "settled_amount in {} is required for a {currency} refund",
                    account.currency
                )))
            }
        };
        positive_amount(settled)?;
        if currency == account.currency && settled != amount {
            return Err(KokuError::InvalidInput(
                "same-currency refunds must settle for the original amount".to_owned(),
            ));
        }
        let new_balance = account.account_type.apply_inflow(account.balance, settled);
        Self::set_balance(&tx, account_id, new_balance)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, category_id, amount, currency, settled_amount, occurred_at, note) VALUES ('income', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                category.id,
                decimal_to_db(amount),
                currency,
                decimal_to_db(settled),
                timestamp(now),
                note.into()
            ],
        )?;
        let income_id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE transactions SET refunded_amount = ?1 WHERE id = ?2",
            params![decimal_to_db(expense.refunded_amount + amount), expense_id],
        )?;
        tx.execute(
            "INSERT INTO refunds(expense_id, income_id, amount, refunded_at) VALUES (?1, ?2, ?3, ?4)",
            params![expense_id, income_id, decimal_to_db(amount), timestamp(now)],
        )?;
        tx.commit()?;
        self.transaction(income_id)
    }
}
