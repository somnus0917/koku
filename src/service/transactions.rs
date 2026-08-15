//! 收支与转账：原子余额更新、软撤销、流水查询。

use chrono::{DateTime, Utc};
use rusqlite::{params, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{CategoryKind, Transaction, TransactionKind};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    pub fn record_expense(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        let currency = self.account(account_id)?.currency;
        self.record_expense_in_currency(
            account_id,
            category_id,
            amount,
            currency,
            amount,
            occurred_at,
            note,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_expense_in_currency(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        settled_amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        self.record_categorized(
            TransactionKind::Expense,
            account_id,
            category_id,
            amount,
            currency.into(),
            settled_amount,
            occurred_at,
            note.into(),
        )
    }

    pub fn record_income(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        let currency = self.account(account_id)?.currency;
        self.record_income_in_currency(
            account_id,
            category_id,
            amount,
            currency,
            amount,
            occurred_at,
            note,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_income_in_currency(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        settled_amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        self.record_categorized(
            TransactionKind::Income,
            account_id,
            category_id,
            amount,
            currency.into(),
            settled_amount,
            occurred_at,
            note.into(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn record_categorized(
        &mut self,
        kind: TransactionKind,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        currency: String,
        settled_amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: String,
    ) -> Result<Transaction> {
        positive_amount(amount)?;
        positive_amount(settled_amount)?;
        let currency = normalize_currency(currency)?;
        let expected_category_kind = match kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            TransactionKind::Transfer => {
                return Err(KokuError::InvalidInput(
                    "categorized transactions cannot be transfers".to_owned(),
                ))
            }
            TransactionKind::Loan | TransactionKind::Adjustment => {
                return Err(KokuError::InvalidInput(
                    "categorized transactions cannot be loans or adjustments".to_owned(),
                ))
            }
        };

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        if currency == account.currency && amount != settled_amount {
            return Err(KokuError::InvalidInput(
                "same-currency transactions must settle for the original amount".to_owned(),
            ));
        }
        let category = Self::category_in_tx(&tx, category_id)?;
        if category.kind != expected_category_kind {
            return Err(KokuError::CategoryKindMismatch {
                expected: expected_category_kind.as_str(),
                actual: category.kind.as_str(),
            });
        }

        let current_balance = account.balance;
        let new_balance = match kind {
            TransactionKind::Expense => account
                .account_type
                .apply_outflow(current_balance, settled_amount),
            TransactionKind::Income => account
                .account_type
                .apply_inflow(current_balance, settled_amount),
            TransactionKind::Transfer | TransactionKind::Loan | TransactionKind::Adjustment => {
                unreachable!("validated above")
            }
        };
        Self::set_balance(&tx, account_id, new_balance)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, category_id, amount, currency, settled_amount, occurred_at, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![kind.as_str(), account_id, category_id, decimal_to_db(amount), currency, decimal_to_db(settled_amount), timestamp(occurred_at), note],
        )?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn record_transfer(
        &mut self,
        from_account_id: i64,
        to_account_id: i64,
        source_amount: Decimal,
        target_amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        if from_account_id == to_account_id {
            return Err(KokuError::InvalidInput(
                "source and target accounts must be different".to_owned(),
            ));
        }
        positive_amount(source_amount)?;
        positive_amount(target_amount)?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = Self::account_in_tx(&tx, from_account_id)?;
        let target = Self::account_in_tx(&tx, to_account_id)?;
        if source.currency == target.currency && source_amount != target_amount {
            return Err(KokuError::InvalidInput(
                "same-currency transfers must use equal source and target amounts".to_owned(),
            ));
        }

        Self::set_balance(
            &tx,
            from_account_id,
            source
                .account_type
                .apply_outflow(source.balance, source_amount),
        )?;
        Self::set_balance(
            &tx,
            to_account_id,
            target
                .account_type
                .apply_inflow(target.balance, target_amount),
        )?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, to_account_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note) VALUES ('transfer', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![from_account_id, to_account_id, decimal_to_db(source_amount), source.currency, decimal_to_db(source_amount), decimal_to_db(target_amount), target.currency, timestamp(occurred_at), note.into()],
        )?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn void_transaction(&mut self, transaction_id: i64) -> Result<Transaction> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        if transaction.voided_at.is_some() {
            return Err(KokuError::AlreadyVoided);
        }

        match transaction.kind {
            TransactionKind::Expense => {
                // 撤销已（部分）报销的支出：级联撤销其报销收入流水并清空报销状态，
                // 避免「余额已按全额恢复、报销收入却仍在账」造成重复入账。
                let reimbursements = Self::reimbursements_for_expense_in_tx(&tx, transaction_id)?;
                for (income_id, _) in &reimbursements {
                    Self::void_reimbursement_income_in_tx(&tx, *income_id)?;
                }
                if !reimbursements.is_empty() {
                    tx.execute(
                        "UPDATE transactions
                         SET reimbursed_amount = '0', reimbursed_at = NULL, reimbursable_at = NULL
                         WHERE id = ?1",
                        [transaction_id],
                    )?;
                }
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source
                        .account_type
                        .apply_inflow(source.balance, transaction.settled_amount),
                )?;
            }
            TransactionKind::Income => {
                // 若这笔收入是某笔支出的报销，回写支出：扣减累计已报销金额，
                // 不再全额报销时清除 reimbursed_at；保留待报销标记以便重新报销。
                if let Some((expense_id, amount)) =
                    Self::reimbursement_for_income_in_tx(&tx, transaction_id)?
                {
                    let expense = Self::transaction_in_tx(&tx, expense_id)?;
                    let new_reimbursed = (expense.reimbursed_amount - amount).max(Decimal::ZERO);
                    let reimbursed_at = if new_reimbursed >= expense.amount {
                        expense.reimbursed_at
                    } else {
                        None
                    };
                    tx.execute(
                        "UPDATE transactions SET reimbursed_amount = ?1, reimbursed_at = ?2 WHERE id = ?3",
                        params![
                            decimal_to_db(new_reimbursed),
                            reimbursed_at.map(timestamp),
                            expense_id
                        ],
                    )?;
                }
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source
                        .account_type
                        .apply_outflow(source.balance, transaction.settled_amount),
                )?;
            }
            TransactionKind::Transfer => {
                let target_id = transaction.to_account_id.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target account".to_owned())
                })?;
                let target_amount = transaction.target_amount.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target amount".to_owned())
                })?;
                transaction.target_currency.as_deref().ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target currency".to_owned())
                })?;
                let target = Self::account_in_tx(&tx, target_id)?;
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source
                        .account_type
                        .apply_inflow(source.balance, transaction.settled_amount),
                )?;
                Self::set_balance(
                    &tx,
                    target_id,
                    target
                        .account_type
                        .apply_outflow(target.balance, target_amount),
                )?;
            }
            TransactionKind::Loan => {
                return Err(KokuError::InvalidInput(
                    "loan transactions cannot be voided; repay or adjust the loan instead"
                        .to_owned(),
                ))
            }
            // 余额调整的撤销：把带符号增量反向应用即可恢复原余额。
            TransactionKind::Adjustment => {
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source.balance - transaction.amount,
                )?;
            }
        }

        tx.execute(
            "UPDATE transactions SET voided_at = ?1 WHERE id = ?2 AND voided_at IS NULL",
            params![timestamp(Utc::now()), transaction_id],
        )?;
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn transaction(&self, id: i64) -> Result<Transaction> {
        let raw = self
            .conn
            .query_row(
                "SELECT id, kind, account_id, to_account_id, category_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note, voided_at, loan_id, reimbursable_at, reimbursed_at, reimbursed_amount FROM transactions WHERE id = ?1",
                [id],
                transaction_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "transaction",
                id,
            })?;
        transaction_from_row(raw)
    }

    /// 分页读取流水，按时间倒序。`limit` 必须为 1..=1000，`offset` 从 0 开始。
    pub fn transactions(&self, limit: u32, offset: u32) -> Result<Vec<Transaction>> {
        if !(1..=1000).contains(&limit) {
            return Err(KokuError::InvalidInput(
                "transactions limit must be between 1 and 1000".to_owned(),
            ));
        }
        let mut statement = self.conn.prepare(
            "SELECT id, kind, account_id, to_account_id, category_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note, voided_at, loan_id, reimbursable_at, reimbursed_at, reimbursed_amount FROM transactions ORDER BY occurred_at DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = statement.query_map(params![limit, offset], transaction_row)?;
        rows.map(|row| transaction_from_row(row?)).collect()
    }
}
