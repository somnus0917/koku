//! 账户与分类：创建/编辑/余额调整/信用额度、分类管理、定期存款。

use chrono::{Duration as ChronoDuration, Utc};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{
    Account, AccountType, BalanceSummary, Category, CategoryKind, DepositSettlement, Transaction,
    DEFAULT_CATEGORIES,
};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    pub fn create_account(
        &mut self,
        name: impl Into<String>,
        account_type: AccountType,
        currency: impl Into<String>,
        opening_balance: Decimal,
    ) -> Result<Account> {
        let name = required_text(name.into(), "account name")?;
        let currency = normalize_currency(currency.into())?;
        let now = timestamp(Utc::now());
        self.conn.execute(
            "INSERT INTO accounts(name, account_type, currency, balance, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, account_type.as_str(), currency, decimal_to_db(opening_balance), now],
        )?;
        self.account(self.conn.last_insert_rowid())
    }

    /// 更新账户名称/类型/币种；有交易历史的账户不允许改币种（避免历史流水语义混乱）。
    pub fn update_account(
        &mut self,
        id: i64,
        name: Option<String>,
        account_type: Option<AccountType>,
        currency: Option<String>,
    ) -> Result<Account> {
        let current = self.account(id)?;
        let name = match name {
            Some(value) => required_text(value, "account name")?,
            None => current.name,
        };
        let account_type = account_type.unwrap_or(current.account_type);
        let currency = match currency {
            Some(value) => {
                let currency = normalize_currency(value)?;
                if currency != current.currency {
                    let count: i64 = self.conn.query_row(
                        "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 OR to_account_id = ?1",
                        [id],
                        |row| row.get(0),
                    )?;
                    if count > 0 {
                        return Err(KokuError::InvalidInput(
                            "cannot change the currency of an account with transaction history"
                                .to_owned(),
                        ));
                    }
                }
                currency
            }
            None => current.currency,
        };
        self.conn.execute(
            "UPDATE accounts SET name = ?1, account_type = ?2, currency = ?3 WHERE id = ?4",
            params![name, account_type.as_str(), currency, id],
        )?;
        self.account(id)
    }

    /// 调整账户余额：`amount` 为带符号增量（正数增加、负数减少，按账户方向生效），
    /// 记录一条 `adjustment` 流水（不计入收支统计），可撤销。
    pub fn adjust_balance(
        &mut self,
        account_id: i64,
        amount: Decimal,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        if amount.is_zero() {
            return Err(KokuError::InvalidInput(
                "balance adjustment amount cannot be zero".to_owned(),
            ));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        let new_balance = account.balance + amount;
        Self::set_balance(&tx, account_id, new_balance)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, occurred_at, note) VALUES ('adjustment', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id,
                decimal_to_db(amount),
                account.currency,
                decimal_to_db(amount),
                timestamp(Utc::now()),
                note.into()
            ],
        )?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    /// 设置或清除信用额度（仅对信用账户有意义）。
    pub fn set_credit_limit(&mut self, id: i64, credit_limit: Option<Decimal>) -> Result<Account> {
        self.conn.execute(
            "UPDATE accounts SET credit_limit = ?1 WHERE id = ?2",
            params![credit_limit.map(decimal_to_db), id],
        )?;
        self.account(id)
    }

    pub fn create_category(
        &mut self,
        name: impl Into<String>,
        kind: CategoryKind,
    ) -> Result<Category> {
        let name = required_text(name.into(), "category name")?;
        self.conn.execute(
            r#"
            INSERT INTO categories(name, kind, created_at, archived_at)
            VALUES (?1, ?2, ?3, NULL)
            ON CONFLICT(name, kind) DO UPDATE SET archived_at = NULL
            "#,
            params![name, kind.as_str(), timestamp(Utc::now())],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM categories WHERE name = ?1 AND kind = ?2",
            params![name, kind.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        self.category(id)
    }

    pub fn ensure_default_categories(&mut self) -> Result<()> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let created_at = timestamp(Utc::now());
        for (name, kind) in DEFAULT_CATEGORIES {
            transaction.execute(
                "INSERT OR IGNORE INTO categories(name, kind, created_at) VALUES (?1, ?2, ?3)",
                params![name, kind.as_str(), created_at],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn account(&self, id: i64) -> Result<Account> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, account_type, currency, balance, interest_rate, maturity_at, credit_limit FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "account",
                id,
            })?;
        account_from_row(row)
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, account_type, currency, balance, interest_rate, maturity_at, credit_limit FROM accounts ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })?;
        rows.map(|row| account_from_row(row?)).collect()
    }

    /// 资产负债汇总：`currency` 为显示币种，所有币种的账户余额与未结借款统一折算。
    pub fn balance_summary(&self, currency: &str) -> Result<BalanceSummary> {
        let currency = normalize_currency(currency.to_owned())?;
        let today = Utc::now().date_naive();
        let mut total_assets = Decimal::ZERO;
        let mut total_liabilities = Decimal::ZERO;
        let mut statement = self
            .conn
            .prepare("SELECT account_type, currency, balance FROM accounts")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (account_type, account_currency, balance) = row?;
            let balance = self.convert_amount(
                decimal_from_db(&balance)?,
                &account_currency,
                &currency,
                today,
            )?;
            let account_type = AccountType::from_db(&account_type)?;
            if account_type.is_liability() {
                total_liabilities += balance;
            } else {
                total_assets += balance;
            }
        }
        // 未结借款纳入净资产：借出 = 应收（资产），借入 = 应付（负债），同样按汇率折算。
        let mut statement = self.conn.prepare(
            "SELECT loan_type, currency, outstanding FROM loans WHERE closed_at IS NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (loan_type, loan_currency, outstanding) = row?;
            let outstanding = self.convert_amount(
                decimal_from_db(&outstanding)?,
                &loan_currency,
                &currency,
                today,
            )?;
            match LoanType::from_db(&loan_type)? {
                LoanType::Lend => total_assets += outstanding,
                LoanType::Borrow => total_liabilities += outstanding,
            }
        }
        // 股票持仓按市值（有市价用市价，否则用摊薄成本）计入资产。
        let mut statement = self.conn.prepare(
            "SELECT a.account_type, a.currency, h.shares, h.cost_basis, h.last_price
             FROM holdings h JOIN accounts a ON a.id = h.account_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (account_type, account_currency, shares, cost_basis, last_price) = row?;
            if AccountType::from_db(&account_type)? != AccountType::Stock {
                continue;
            }
            let shares = decimal_from_db(&shares)?;
            let cost_basis = decimal_from_db(&cost_basis)?;
            let last_price = last_price.as_deref().map(decimal_from_db).transpose()?;
            let per_share = last_price.unwrap_or_else(|| {
                if shares.is_zero() {
                    Decimal::ZERO
                } else {
                    cost_basis / shares
                }
            });
            let market_value = (shares * per_share).round_dp(2);
            total_assets +=
                self.convert_amount(market_value, &account_currency, &currency, today)?;
        }
        Ok(BalanceSummary {
            currency,
            total_assets,
            total_liabilities,
            net_worth: total_assets - total_liabilities,
        })
    }

    /// 资产负债涉及的所有币种（账户币种 ∪ 未结借款币种），供调用方确保折算汇率可用。
    pub fn balance_currencies(&self) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT currency FROM accounts
             UNION
             SELECT DISTINCT currency FROM loans WHERE closed_at IS NULL",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut currencies = Vec::new();
        for row in rows {
            currencies.push(row?);
        }
        Ok(currencies)
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(count == 0)
    }

    pub fn category(&self, id: i64) -> Result<Category> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, kind FROM categories WHERE id = ?1 AND archived_at IS NULL",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "category",
                id,
            })?;
        category_from_row(row)
    }

    pub fn categories(&self) -> Result<Vec<Category>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, kind FROM categories WHERE archived_at IS NULL ORDER BY kind, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| category_from_row(row?)).collect()
    }

    pub fn delete_category(&mut self, id: i64) -> Result<Category> {
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let category = Self::category_in_tx(&transaction, id)?;
        transaction.execute(
            "UPDATE categories SET archived_at = ?1 WHERE id = ?2 AND archived_at IS NULL",
            params![timestamp(Utc::now()), id],
        )?;
        transaction.commit()?;
        Ok(category)
    }

    /// 把储蓄账户中的一笔钱转为定期：自动创建带利率和到期日的定期账户并原子转账。
    pub fn create_fixed_deposit(
        &mut self,
        from_account_id: i64,
        amount: Decimal,
        currency: impl Into<String>,
        rate: Decimal,
        term_days: u32,
        note: impl Into<String>,
    ) -> Result<Account> {
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
        let maturity = now + ChronoDuration::days(term_days as i64);
        let name = format!("定期·{term_days}天 {rate}%");
        let deposit =
            self.create_account(name, AccountType::Savings, currency.clone(), Decimal::ZERO)?;
        self.conn.execute(
            "UPDATE accounts SET interest_rate = ?1, maturity_at = ?2 WHERE id = ?3",
            params![decimal_to_db(rate), timestamp(maturity), deposit.id],
        )?;
        self.record_transfer(from_account_id, deposit.id, amount, amount, now, note)?;
        self.account(deposit.id)
    }

    /// 结清定期：按实际持有天数计算利息（记一笔利息收入），再把本息转回目标账户。
    pub fn settle_deposit(
        &mut self,
        deposit_id: i64,
        to_account_id: i64,
    ) -> Result<DepositSettlement> {
        let deposit = self.account(deposit_id)?;
        let Some(rate) = deposit.interest_rate else {
            return Err(KokuError::InvalidInput(
                "account is not a fixed deposit".to_owned(),
            ));
        };
        if deposit.balance <= Decimal::ZERO {
            return Err(KokuError::InvalidInput(
                "deposit has no balance left to settle".to_owned(),
            ));
        }
        let target = self.account(to_account_id)?;
        if target.currency != deposit.currency {
            return Err(KokuError::InvalidInput(
                "settlement target must use the same currency as the deposit".to_owned(),
            ));
        }
        let created_at: String = self.conn.query_row(
            "SELECT created_at FROM accounts WHERE id = ?1",
            [deposit_id],
            |row| row.get(0),
        )?;
        let start = parse_timestamp(&created_at)?;
        let days = (Utc::now() - start).num_days().max(0);
        let hundred = Decimal::from(100_u32);
        let year = Decimal::from(365_u32);
        let interest = (deposit.balance * rate / hundred * Decimal::from(days) / year).round_dp(2);
        let now = Utc::now();
        if interest > Decimal::ZERO {
            let interest_category = self.create_category("利息", CategoryKind::Income)?;
            self.record_income_in_currency(
                deposit_id,
                interest_category.id,
                interest,
                deposit.currency.clone(),
                interest,
                now,
                "定期利息",
            )?;
        }
        let final_balance = self.account(deposit_id)?.balance;
        let transfer = self.record_transfer(
            deposit_id,
            to_account_id,
            final_balance,
            final_balance,
            now,
            "定期到期转回",
        )?;
        Ok(DepositSettlement { interest, transfer })
    }
}
