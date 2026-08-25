//! 借款：借出/借入、跨账户还款、未结余额与结清。

use chrono::{DateTime, Utc};
use rusqlite::{params, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{CategoryKind, Loan, LoanType, TransactionKind};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 借出/借入：创建借款记录并从账户划拨本金（借出扣减余额、借入增加余额）。
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn create_loan(
        &mut self,
        loan_type: LoanType,
        counterparty: impl Into<String>,
        currency: impl Into<String>,
        amount: Decimal,
        account_id: i64,
        note: impl Into<String>,
        due_at: Option<DateTime<Utc>>,
    ) -> Result<Loan> {
        self.create_loan_with_interest(
            loan_type,
            counterparty,
            currency,
            amount,
            account_id,
            note,
            due_at,
            None,
        )
    }

    /// 创建可选计息借款；有息借款保持独立，以免不同放款日期被错误合并计息。
    #[allow(clippy::too_many_arguments)]
    pub fn create_loan_with_interest(
        &mut self,
        loan_type: LoanType,
        counterparty: impl Into<String>,
        currency: impl Into<String>,
        amount: Decimal,
        account_id: i64,
        note: impl Into<String>,
        due_at: Option<DateTime<Utc>>,
        interest_rate: Option<Decimal>,
    ) -> Result<Loan> {
        let counterparty = required_text(counterparty.into(), "counterparty")?;
        positive_amount(amount)?;
        let interest_rate = match interest_rate {
            Some(rate) if rate < Decimal::ZERO => {
                return Err(KokuError::InvalidInput(
                    "loan interest rate cannot be negative".to_owned(),
                ));
            }
            Some(rate) if rate.is_zero() => None,
            value => value,
        };
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
        let existing_id = if interest_rate.is_none() {
            Self::open_loan_id(&tx, loan_type, &counterparty, &currency)?
        } else {
            None
        };
        let loan_id = if let Some(existing_id) = existing_id {
            let (principal, outstanding) = Self::loan_totals(&tx, existing_id)?;
            tx.execute(
                "UPDATE loans SET principal = ?1, outstanding = ?2 WHERE id = ?3",
                params![
                    decimal_to_db(principal + amount),
                    decimal_to_db(outstanding + amount),
                    existing_id
                ],
            )?;
            if let Some(due) = due_at {
                tx.execute(
                    "UPDATE loans SET due_at = ?1 WHERE id = ?2",
                    params![timestamp(due), existing_id],
                )?;
            }
            existing_id
        } else {
            tx.execute(
                "INSERT INTO loans(loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, due_at, interest_rate) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    loan_type.as_str(),
                    &counterparty,
                    currency,
                    decimal_to_db(amount),
                    decimal_to_db(amount),
                    account_id,
                    timestamp(now),
                    note.into(),
                    due_at.map(timestamp),
                    interest_rate.map(decimal_to_db)
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
            "SELECT id FROM loans WHERE loan_type = ?1 AND counterparty = ?2 AND currency = ?3 AND closed_at IS NULL AND interest_rate IS NULL ORDER BY id LIMIT 1",
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
        let preview = self.loan(loan_id)?;
        if preview.closed_at.is_some() {
            return Err(KokuError::InvalidInput(
                "loan is already settled".to_owned(),
            ));
        }
        if preview.interest_rate.is_some() && amount != preview.outstanding {
            return Err(KokuError::InvalidInput(
                "interest-bearing loans must be settled in full".to_owned(),
            ));
        }
        let now = Utc::now();
        let interest = preview
            .interest_rate
            .map(|rate| calculate_simple_interest(preview.principal, rate, preview.opened_at, now))
            .unwrap_or(Decimal::ZERO);
        let interest_category = if interest > Decimal::ZERO {
            let kind = match preview.loan_type {
                LoanType::Lend => CategoryKind::Income,
                LoanType::Borrow => CategoryKind::Expense,
            };
            Some(self.create_category("利息", kind)?.id)
        } else {
            None
        };
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
        let settled_interest = if interest.is_zero() {
            Decimal::ZERO
        } else if currency == account.currency {
            interest
        } else {
            (interest * settled / amount).round_dp(2)
        };
        let total_settled = settled + settled_interest;
        let new_balance = match loan.loan_type {
            LoanType::Lend => account
                .account_type
                .apply_inflow(account.balance, total_settled),
            LoanType::Borrow => account
                .account_type
                .apply_outflow(account.balance, total_settled),
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
                timestamp(now),
                txn_note
            ],
        )?;
        if let Some(category_id) = interest_category {
            let kind = match loan.loan_type {
                LoanType::Lend => TransactionKind::Income,
                LoanType::Borrow => TransactionKind::Expense,
            };
            tx.execute(
                "INSERT INTO transactions(kind, account_id, category_id, amount, currency, settled_amount, loan_id, occurred_at, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    kind.as_str(),
                    account_id,
                    category_id,
                    decimal_to_db(interest),
                    &loan.currency,
                    decimal_to_db(settled_interest),
                    loan_id,
                    timestamp(now),
                    format!("{}利息 {}", loan.loan_type.label(), loan.counterparty)
                ],
            )?;
        }
        let outstanding = loan.outstanding - amount;
        let closed_at = if outstanding.is_zero() {
            Some(timestamp(now))
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
            "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at, due_at, interest_rate FROM loans ORDER BY id DESC",
        )?;
        let rows = statement.query_map([], loan_row)?;
        rows.map(|row| loan_from_row(row?)).collect()
    }

    pub fn loan(&self, id: i64) -> Result<Loan> {
        let row = self
            .conn
            .query_row(
                "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at, due_at, interest_rate FROM loans WHERE id = ?1",
                [id],
                loan_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound { entity: "loan", id })?;
        loan_from_row(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use chrono::{Datelike, Duration};

    #[test]
    fn interest_bearing_loan_accrues_and_posts_interest_on_full_settlement() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let cash =
            service.create_account("现金", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let loan = service.create_loan_with_interest(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(1000_u32),
            cash.id,
            "一年期",
            None,
            Some(Decimal::from(5_u32)),
        )?;
        let opened_at = Utc::now() - Duration::days(365);
        service.conn.execute(
            "UPDATE loans SET opened_at = ?1 WHERE id = ?2",
            params![timestamp(opened_at), loan.id],
        )?;

        let accrued = service.loan(loan.id)?;
        assert_eq!(accrued.interest_rate, Some(Decimal::from(5_u32)));
        assert_eq!(accrued.accrued_interest, Decimal::from(50_u32));
        assert!(service
            .repay_loan(
                loan.id,
                cash.id,
                Decimal::from(500_u32),
                "CNY",
                None,
                "部分还款",
            )
            .is_err());

        let settled = service.repay_loan(
            loan.id,
            cash.id,
            Decimal::from(1000_u32),
            "CNY",
            None,
            "结清",
        )?;
        assert!(settled.closed_at.is_some());
        assert_eq!(settled.accrued_interest, Decimal::from(50_u32));
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(1050_u32));
        let now = Utc::now();
        assert_eq!(
            service
                .monthly_summary(now.year(), now.month(), "CNY")?
                .total_income,
            Decimal::from(50_u32)
        );
        Ok(())
    }

    #[test]
    fn interest_bearing_loans_do_not_merge() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let cash =
            service.create_account("现金", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let first = service.create_loan_with_interest(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(100_u32),
            cash.id,
            "第一笔",
            None,
            Some(Decimal::from(5_u32)),
        )?;
        let second = service.create_loan_with_interest(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(100_u32),
            cash.id,
            "第二笔",
            None,
            Some(Decimal::from(5_u32)),
        )?;
        assert_ne!(first.id, second.id);
        Ok(())
    }
}
