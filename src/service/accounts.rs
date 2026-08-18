//! 账户与分类：创建/编辑/余额调整/信用额度、分类管理、定期存款。

use chrono::Utc;
use rust_decimal::Decimal;

use super::*;
use crate::domain::{
    Account, AccountType, BalanceSummary, Category, CategoryKind, LoanType, Transaction,
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
    ///
    /// 便捷包装：不涉及信用卡字段；生产路径统一走
    /// [`BookkeepingService::update_account_edit`]（本方法供测试与
    /// 明确「只改基础字段」的调用方使用，二进制构建中仅测试引用）。
    #[allow(dead_code)]
    pub fn update_account(
        &mut self,
        id: i64,
        name: Option<String>,
        account_type: Option<AccountType>,
        currency: Option<String>,
    ) -> Result<Account> {
        self.update_account_edit(id, name, account_type, currency, None, None, None)
    }

    /// 一次编辑账户请求的全部字段在同一个事务内原子提交（生产统一入口）。
    ///
    /// 覆盖 name / account_type / currency / credit_limit / statement_day / due_day。
    ///
    /// 语义：
    /// - 基础字段 `None` = 不修改；
    /// - `credit_limit` / `statement_day` / `due_day` 为
    ///   `Option<Option<...>>`：`None` = 不修改，`Some(Some(值))` = 设置，
    ///   `Some(None)` = 清除；
    /// - 这三个字段**设置非空值**时，最终账户类型必须为 Credit，否则报错；
    /// - Credit → 非 Credit 时自动清除 credit_limit / statement_day / due_day；
    /// - 有交易历史时禁止 Credit ↔ 非 Credit 类型切换（余额语义会被翻转）；
    /// - 任一步校验失败整体回滚，绝不出现部分字段落库。
    #[allow(clippy::too_many_arguments)]
    pub fn update_account_edit(
        &mut self,
        id: i64,
        name: Option<String>,
        account_type: Option<AccountType>,
        currency: Option<String>,
        credit_limit: Option<Option<Decimal>>,
        statement_day: Option<Option<u32>>,
        due_day: Option<Option<u32>>,
    ) -> Result<Account> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = Self::account_in_tx(&tx, id)?;
        let name = match name {
            Some(value) => required_text(value, "account name")?,
            None => current.name,
        };
        let final_type = account_type.unwrap_or(current.account_type);
        let currency = match currency {
            Some(value) => {
                let currency = normalize_currency(value)?;
                if currency != current.currency {
                    let count: i64 = tx.query_row(
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

        // 信用卡字段：设置非空值时最终类型必须为 Credit。
        let sets_credit_limit = matches!(credit_limit, Some(Some(_)));
        let sets_statement_day = matches!(statement_day, Some(Some(_)));
        let sets_due_day = matches!(due_day, Some(Some(_)));
        if (sets_credit_limit || sets_statement_day || sets_due_day)
            && final_type != AccountType::Credit
        {
            return Err(KokuError::InvalidInput(
                "credit limit, statement day, and due day are only available for credit accounts"
                    .to_owned(),
            ));
        }
        if let Some(Some(day)) = statement_day {
            validate_credit_day(Some(day), "statement day")?;
        }
        if let Some(Some(day)) = due_day {
            validate_credit_day(Some(day), "due day")?;
        }
        // 类型切换：有交易历史时禁止 Credit ↔ 非 Credit（余额语义翻转）。
        if final_type != current.account_type
            && current.account_type.is_liability() != final_type.is_liability()
        {
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM transactions WHERE account_id = ?1 OR to_account_id = ?1",
                [id],
                |row| row.get(0),
            )?;
            if count > 0 {
                return Err(KokuError::InvalidInput(
                    "cannot switch between credit and non-credit for an account with transaction history"
                        .to_owned(),
                ));
            }
        }

        // 最终信用卡字段：非 Credit 一律清除；Credit 按提供值或原值。
        let final_credit_limit = if final_type == AccountType::Credit {
            match credit_limit {
                Some(value) => value,
                None => current.credit_limit,
            }
        } else {
            None
        };
        let final_statement_day: Option<i64> = if final_type == AccountType::Credit {
            match statement_day {
                Some(value) => value.map(i64::from),
                None => current.statement_day.map(i64::from),
            }
        } else {
            None
        };
        let final_due_day: Option<i64> = if final_type == AccountType::Credit {
            match due_day {
                Some(value) => value.map(i64::from),
                None => current.due_day.map(i64::from),
            }
        } else {
            None
        };

        tx.execute(
            "UPDATE accounts
             SET name = ?1, account_type = ?2, currency = ?3,
                 credit_limit = ?4, statement_day = ?5, due_day = ?6
             WHERE id = ?7",
            params![
                name,
                final_type.as_str(),
                currency,
                final_credit_limit.map(decimal_to_db),
                final_statement_day,
                final_due_day,
                id
            ],
        )?;
        tx.commit()?;
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

    /// 设置或清除账单日（1~31，可空；仅对信用账户有意义）。
    ///
    /// 生产路径统一走 [`BookkeepingService::update_account_edit`]；本方法作为
    /// 便捷设置器供测试使用（二进制构建中仅测试引用）。
    #[allow(dead_code)]
    pub fn set_statement_day(&mut self, id: i64, statement_day: Option<u32>) -> Result<Account> {
        validate_credit_day(statement_day, "statement day")?;
        self.conn.execute(
            "UPDATE accounts SET statement_day = ?1 WHERE id = ?2",
            params![statement_day.map(i64::from), id],
        )?;
        self.account(id)
    }

    /// 设置或清除还款日（1~31，可空；仅对信用账户有意义）。
    ///
    /// 生产路径统一走 [`BookkeepingService::update_account_edit`]；本方法作为
    /// 便捷设置器供测试使用（二进制构建中仅测试引用）。
    #[allow(dead_code)]
    pub fn set_due_day(&mut self, id: i64, due_day: Option<u32>) -> Result<Account> {
        validate_credit_day(due_day, "due day")?;
        self.conn.execute(
            "UPDATE accounts SET due_day = ?1 WHERE id = ?2",
            params![due_day.map(i64::from), id],
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
                "SELECT id, name, account_type, currency, balance, credit_limit, statement_day, due_day FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
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
            "SELECT id, name, account_type, currency, balance, credit_limit, statement_day, due_day FROM accounts ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
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
        // 未结清的定期本金计入资产（本金已不在任何账户余额里）。
        let mut statement = self
            .conn
            .prepare("SELECT currency, amount FROM deposits WHERE settled_at IS NULL")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (deposit_currency, amount) = row?;
            total_assets += self.convert_amount(
                decimal_from_db(&amount)?,
                &deposit_currency,
                &currency,
                today,
            )?;
        }
        Ok(BalanceSummary {
            currency,
            total_assets,
            total_liabilities,
            net_worth: total_assets - total_liabilities,
        })
    }

    /// 资产负债涉及的所有币种（账户 ∪ 未结借款 ∪ 未结定期），供调用方确保折算汇率可用。
    pub fn balance_currencies(&self) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT currency FROM accounts
             UNION
             SELECT DISTINCT currency FROM loans WHERE closed_at IS NULL
             UNION
             SELECT DISTINCT currency FROM deposits WHERE settled_at IS NULL",
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
}

/// 校验账单日/还款日：`None`（清除）或 1~31；越界报错。
fn validate_credit_day(day: Option<u32>, field: &str) -> Result<()> {
    if let Some(day) = day {
        if !(1..=31).contains(&day) {
            return Err(KokuError::InvalidInput(format!(
                "{field} must be between 1 and 31"
            )));
        }
    }
    Ok(())
}
