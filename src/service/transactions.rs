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
        let expected_category_kind =
            match kind {
                TransactionKind::Expense => CategoryKind::Expense,
                TransactionKind::Income => CategoryKind::Income,
                TransactionKind::Transfer => {
                    return Err(KokuError::InvalidInput(
                        "categorized transactions cannot be transfers".to_owned(),
                    ))
                }
                TransactionKind::Loan
                | TransactionKind::Adjustment
                | TransactionKind::Trade
                | TransactionKind::Deposit => return Err(KokuError::InvalidInput(
                    "categorized transactions cannot be loans, adjustments, trades, or deposits"
                        .to_owned(),
                )),
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
            TransactionKind::Transfer
            | TransactionKind::Loan
            | TransactionKind::Adjustment
            | TransactionKind::Trade
            | TransactionKind::Deposit => {
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
            TransactionKind::Trade => {
                return Err(KokuError::InvalidInput(
                    "trade transactions cannot be voided; record an opposite trade instead"
                        .to_owned(),
                ))
            }
            TransactionKind::Deposit => {
                return Err(KokuError::InvalidInput(
                    "deposit transactions cannot be voided; settle the deposit instead".to_owned(),
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

    /// 撤销删除：恢复一笔已撤销的流水，重新应用其余额影响并清除 voided_at。
    ///
    /// 与 `void_transaction` 完全对称：报销支出会一并恢复其级联撤销的报销收入
    /// 流水并重建报销状态，报销收入会回写所属支出的累计已报销金额。
    pub fn restore_transaction(&mut self, transaction_id: i64) -> Result<Transaction> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        if transaction.voided_at.is_none() {
            return Err(KokuError::NotVoided);
        }

        match transaction.kind {
            TransactionKind::Expense => {
                // 恢复撤销支出时级联撤销的报销收入，并重建报销状态。
                let reimbursements = Self::reimbursements_for_expense_in_tx(&tx, transaction_id)?;
                let mut reimbursed = Decimal::ZERO;
                for (income_id, amount) in &reimbursements {
                    Self::restore_reimbursement_income_in_tx(&tx, *income_id)?;
                    reimbursed += *amount;
                }
                if !reimbursements.is_empty() {
                    let fully = reimbursed >= transaction.amount;
                    tx.execute(
                        "UPDATE transactions
                         SET reimbursed_amount = ?1, reimbursed_at = ?2, reimbursable_at = ?3
                         WHERE id = ?4",
                        params![
                            decimal_to_db(reimbursed),
                            fully.then(|| timestamp(Utc::now())),
                            (!fully).then(|| timestamp(Utc::now())),
                            transaction_id
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
            TransactionKind::Income => {
                // 若这笔收入是某笔支出的报销，回写支出的累计已报销金额。
                if let Some((expense_id, amount)) =
                    Self::reimbursement_for_income_in_tx(&tx, transaction_id)?
                {
                    let expense = Self::transaction_in_tx(&tx, expense_id)?;
                    let new_reimbursed = expense.reimbursed_amount + amount;
                    let reimbursed_at = if new_reimbursed >= expense.amount {
                        Some(Utc::now())
                    } else {
                        None
                    };
                    tx.execute(
                        "UPDATE transactions SET reimbursed_amount = ?1, reimbursed_at = ?2 WHERE id = ?3",
                        params![decimal_to_db(new_reimbursed), reimbursed_at.map(timestamp), expense_id],
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
            TransactionKind::Transfer => {
                let target_id = transaction.to_account_id.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target account".to_owned())
                })?;
                let target_amount = transaction.target_amount.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target amount".to_owned())
                })?;
                let target = Self::account_in_tx(&tx, target_id)?;
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source
                        .account_type
                        .apply_outflow(source.balance, transaction.settled_amount),
                )?;
                Self::set_balance(
                    &tx,
                    target_id,
                    target
                        .account_type
                        .apply_inflow(target.balance, target_amount),
                )?;
            }
            TransactionKind::Adjustment => {
                let source = Self::account_in_tx(&tx, transaction.account_id)?;
                Self::set_balance(
                    &tx,
                    transaction.account_id,
                    source.balance + transaction.amount,
                )?;
            }
            // 借款/股票/定期流水不允许撤销，因此也永远到不了这里；防御性报错。
            TransactionKind::Loan | TransactionKind::Trade | TransactionKind::Deposit => {
                return Err(KokuError::InvalidInput(
                    "loan, trade, and deposit transactions cannot be restored".to_owned(),
                ))
            }
        }

        tx.execute(
            "UPDATE transactions SET voided_at = NULL WHERE id = ?1 AND voided_at IS NOT NULL",
            [transaction_id],
        )?;
        tx.commit()?;
        self.transaction(transaction_id)
    }

    /// 永久删除一笔已撤销的流水（连带小票、标签与报销关联）。
    ///
    /// 只允许删除已撤销的流水：撤销时余额已恢复，删除不会改动任何余额。
    /// 删除支出会级联永久删除其报销收入流水（均已随支出级联撤销）；若某笔
    /// 报销收入被单独恢复过，则保留该笔收入、仅解除报销关联。
    pub fn delete_transaction(&mut self, transaction_id: i64) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        if transaction.voided_at.is_none() {
            return Err(KokuError::NotVoided);
        }
        Self::delete_transaction_in_tx(&tx, transaction_id)?;
        tx.commit()?;
        Ok(())
    }

    /// 删除单笔流水及其依赖（小票、标签、报销关联）；支出级联删除已撤销的报销收入。
    fn delete_transaction_in_tx(tx: &SqlTransaction<'_>, transaction_id: i64) -> Result<()> {
        let transaction = Self::transaction_in_tx(tx, transaction_id)?;
        if matches!(transaction.kind, TransactionKind::Expense) {
            let links = Self::reimbursements_for_expense_in_tx(tx, transaction_id)?;
            for (income_id, _) in links {
                // 仅级联删除已撤销的报销收入；被单独恢复的收入保留为普通收入。
                if Self::transaction_in_tx(tx, income_id)?.voided_at.is_some() {
                    Self::delete_transaction_in_tx(tx, income_id)?;
                }
            }
        }
        tx.execute(
            "DELETE FROM receipts WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        tx.execute(
            "DELETE FROM transaction_tags WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        // 永久删除必须同步撤销该交易贡献的学习样本（幽灵样本防护）。
        Self::revoke_transaction_learning_in_tx(tx, transaction_id)?;
        tx.execute(
            "DELETE FROM reimbursements WHERE expense_id = ?1 OR income_id = ?1",
            [transaction_id],
        )?;
        tx.execute("DELETE FROM transactions WHERE id = ?1", [transaction_id])?;
        Ok(())
    }

    /// 编辑一笔收入/支出流水：原子地撤销旧余额影响并应用新影响。
    ///
    /// 可改字段：备注、时间、分类、金额、账户（须与旧账户同币种）、结算额。
    /// 不可改：已撤销的流水、转账/借款/调整流水、已发生报销的支出、报销收入
    /// 流水的金额/账户/结算额（这些只允许改备注/分类/时间）。
    #[allow(clippy::too_many_arguments)]
    pub fn update_transaction(
        &mut self,
        transaction_id: i64,
        note: Option<String>,
        occurred_at: Option<DateTime<Utc>>,
        category_id: Option<i64>,
        amount: Option<Decimal>,
        account_id: Option<i64>,
        settled_amount: Option<Decimal>,
    ) -> Result<Transaction> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;

        if transaction.voided_at.is_some() {
            return Err(KokuError::InvalidInput(
                "voided transactions cannot be edited".to_owned(),
            ));
        }
        if !matches!(
            transaction.kind,
            TransactionKind::Expense | TransactionKind::Income
        ) {
            return Err(KokuError::InvalidInput(
                "only expense and income transactions can be edited".to_owned(),
            ));
        }
        // 已发生报销的支出，或报销产生的收入流水：金额/账户/结算额不可改。
        let reimbursement_linked = Self::reimbursement_for_income_in_tx(&tx, transaction_id)?
            .is_some()
            || !transaction.reimbursed_amount.is_zero();
        if reimbursement_linked
            && (amount.is_some() || account_id.is_some() || settled_amount.is_some())
        {
            return Err(KokuError::InvalidInput(
                "reimbursed transactions can only edit note, category, or time".to_owned(),
            ));
        }

        let new_amount = amount.unwrap_or(transaction.amount);
        positive_amount(new_amount)?;
        let new_account_id = account_id.unwrap_or(transaction.account_id);

        let old_account = Self::account_in_tx(&tx, transaction.account_id)?;
        let new_account = if new_account_id == transaction.account_id {
            old_account.clone()
        } else {
            let account = Self::account_in_tx(&tx, new_account_id)?;
            if account.currency != old_account.currency {
                return Err(KokuError::InvalidInput(format!(
                    "cannot move a transaction to an account with a different currency ({} != {})",
                    old_account.currency, account.currency
                )));
            }
            account
        };
        // 结算额：同币种交易恒等于金额（显式给出且不一致则报错）；外币交易需显式
        // 给出，且改金额时必须一并提供。
        let new_settled = if transaction.currency == new_account.currency {
            if let Some(settled) = settled_amount {
                if settled != new_amount {
                    return Err(KokuError::InvalidInput(
                        "same-currency transactions must settle for the original amount".to_owned(),
                    ));
                }
            }
            new_amount
        } else {
            if amount.is_some() && settled_amount.is_none() {
                return Err(KokuError::InvalidInput(format!(
                    "settled_amount in {} is required when changing the amount of a foreign-currency transaction",
                    new_account.currency
                )));
            }
            let settled = settled_amount.unwrap_or(transaction.settled_amount);
            positive_amount(settled)?;
            settled
        };
        if let Some(category_id) = category_id {
            let category = Self::category_in_tx(&tx, category_id)?;
            let expected = match transaction.kind {
                TransactionKind::Expense => CategoryKind::Expense,
                TransactionKind::Income => CategoryKind::Income,
                _ => unreachable!(),
            };
            if category.kind != expected {
                return Err(KokuError::CategoryKindMismatch {
                    expected: expected.as_str(),
                    actual: category.kind.as_str(),
                });
            }
        }

        // 撤销旧影响（按旧账户类型）→ 应用新影响（按新账户类型）。
        let undo_old = |balance: Decimal| match transaction.kind {
            TransactionKind::Expense => old_account
                .account_type
                .apply_inflow(balance, transaction.settled_amount),
            TransactionKind::Income => old_account
                .account_type
                .apply_outflow(balance, transaction.settled_amount),
            _ => unreachable!(),
        };
        let apply_new = |balance: Decimal| match transaction.kind {
            TransactionKind::Expense => {
                new_account.account_type.apply_outflow(balance, new_settled)
            }
            TransactionKind::Income => new_account.account_type.apply_inflow(balance, new_settled),
            _ => unreachable!(),
        };
        if new_account_id == transaction.account_id {
            let balance = apply_new(undo_old(old_account.balance));
            Self::set_balance(&tx, transaction.account_id, balance)?;
        } else {
            Self::set_balance(&tx, transaction.account_id, undo_old(old_account.balance))?;
            Self::set_balance(&tx, new_account_id, apply_new(new_account.balance))?;
        }

        tx.execute(
            "UPDATE transactions
             SET note = ?1, occurred_at = ?2, category_id = ?3,
                 amount = ?4, account_id = ?5, settled_amount = ?6
             WHERE id = ?7",
            params![
                note.unwrap_or(transaction.note),
                timestamp(occurred_at.unwrap_or(transaction.occurred_at)),
                category_id.or(transaction.category_id),
                decimal_to_db(new_amount),
                new_account_id,
                decimal_to_db(new_settled),
                transaction_id
            ],
        )?;
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn transaction(&self, id: i64) -> Result<Transaction> {
        let raw = self
            .conn
            .query_row(
                "SELECT id, kind, account_id, to_account_id, category_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note, voided_at, loan_id, reimbursable_at, reimbursed_at, reimbursed_amount, EXISTS(SELECT 1 FROM receipts r WHERE r.transaction_id = transactions.id) AS has_receipt, COALESCE((SELECT group_concat(t.name, ',') FROM tags t JOIN transaction_tags tt ON tt.tag_id = t.id WHERE tt.transaction_id = transactions.id), '') AS tags, payee_id, raw_description, (SELECT p.name FROM payees p WHERE p.id = transactions.payee_id) AS payee_name FROM transactions WHERE id = ?1",
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

    /// 分页读取全部流水，按时间倒序。`limit` 必须为 1..=1000，`offset` 从 0 开始。
    pub fn transactions(&self, limit: u32, offset: u32) -> Result<Vec<Transaction>> {
        self.transactions_in_range(None, limit, offset)
    }

    /// 分页读取某自然月的流水（时间倒序）。区间语义复用 `month_bounds`，
    /// 与月度汇总保持一致：`occurred_at >= 当月 0 点` 且 `< 下月 0 点`。
    pub fn transactions_in_month(
        &self,
        year: i32,
        month: u32,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Transaction>> {
        let (start, end) = month_bounds(year, month)?;
        self.transactions_in_range(Some((start, end)), limit, offset)
    }

    fn transactions_in_range(
        &self,
        range: Option<(DateTime<Utc>, DateTime<Utc>)>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Transaction>> {
        if !(1..=1000).contains(&limit) {
            return Err(KokuError::InvalidInput(
                "transactions limit must be between 1 and 1000".to_owned(),
            ));
        }
        const COLUMNS: &str = "SELECT id, kind, account_id, to_account_id, category_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note, voided_at, loan_id, reimbursable_at, reimbursed_at, reimbursed_amount, EXISTS(SELECT 1 FROM receipts r WHERE r.transaction_id = transactions.id) AS has_receipt, COALESCE((SELECT group_concat(t.name, ',') FROM tags t JOIN transaction_tags tt ON tt.tag_id = t.id WHERE tt.transaction_id = transactions.id), '') AS tags, payee_id, raw_description, (SELECT p.name FROM payees p WHERE p.id = transactions.payee_id) AS payee_name FROM transactions";
        let rows: Vec<rusqlite::Result<TransactionRow>> = match range {
            Some((start, end)) => {
                let mut statement = self.conn.prepare(&format!(
                    "{COLUMNS} WHERE occurred_at >= ?1 AND occurred_at < ?2 ORDER BY occurred_at DESC, id DESC LIMIT ?3 OFFSET ?4"
                ))?;
                let mapped = statement.query_map(
                    params![timestamp(start), timestamp(end), limit, offset],
                    transaction_row,
                )?;
                mapped.collect()
            }
            None => {
                let mut statement = self.conn.prepare(&format!(
                    "{COLUMNS} ORDER BY occurred_at DESC, id DESC LIMIT ?1 OFFSET ?2"
                ))?;
                let mapped = statement.query_map(params![limit, offset], transaction_row)?;
                mapped.collect()
            }
        };
        rows.into_iter()
            .map(|row| transaction_from_row(row?))
            .collect()
    }
}
