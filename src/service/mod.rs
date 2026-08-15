//! SQLite 持久化与记账业务：原子余额更新、软撤销、统计与旧库迁移。

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction as SqlTransaction, TransactionBehavior,
};
use rust_decimal::Decimal;

use crate::auth::{generate_session_token, session_token_hash};
use crate::domain::{
    Account, AccountType, CashFlowItem, Category, CategoryKind, Loan, LoanType, Transaction,
    TransactionKind,
};
use crate::error::{KokuError, Result};

pub struct BookkeepingService {
    conn: Connection,
}

mod accounts;
mod loans;
mod reimbursements;
mod summaries;
mod transactions;

impl BookkeepingService {
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        // WAL + synchronous=NORMAL 把每次提交的 fsync 从 2 次降到 1 次（实测写入约 3 倍提速），
        // 并让读不阻塞写；journal_mode 会持久化在数据库文件中，首次打开旧库会自动切换。
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS accounts (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                name          TEXT NOT NULL UNIQUE,
                account_type  TEXT NOT NULL CHECK (account_type IN ('cash', 'credit', 'savings', 'stock')),
                currency      TEXT NOT NULL,
                balance       TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                interest_rate TEXT,
                maturity_at   TEXT,
                credit_limit  TEXT
            );

            CREATE TABLE IF NOT EXISTS categories (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                kind       TEXT NOT NULL CHECK (kind IN ('expense', 'income')),
                created_at TEXT NOT NULL,
                archived_at TEXT,
                UNIQUE(name, kind)
            );

            CREATE TABLE IF NOT EXISTS loans (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                loan_type    TEXT NOT NULL CHECK (loan_type IN ('lend', 'borrow')),
                counterparty TEXT NOT NULL,
                currency     TEXT NOT NULL,
                principal    TEXT NOT NULL,
                outstanding  TEXT NOT NULL,
                account_id   INTEGER NOT NULL REFERENCES accounts(id),
                opened_at    TEXT NOT NULL,
                note         TEXT NOT NULL DEFAULT '',
                closed_at    TEXT
            );

            CREATE TABLE IF NOT EXISTS reimbursements (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                expense_id    INTEGER NOT NULL REFERENCES transactions(id),
                income_id     INTEGER NOT NULL REFERENCES transactions(id),
                amount        TEXT NOT NULL,
                reimbursed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transactions (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'loan', 'adjustment')),
                account_id    INTEGER NOT NULL REFERENCES accounts(id),
                to_account_id INTEGER REFERENCES accounts(id),
                category_id   INTEGER REFERENCES categories(id),
                amount        TEXT NOT NULL,
                currency      TEXT,
                settled_amount TEXT,
                target_amount TEXT,
                target_currency TEXT,
                occurred_at   TEXT NOT NULL,
                note          TEXT NOT NULL DEFAULT '',
                voided_at     TEXT,
                loan_id       INTEGER REFERENCES loans(id),
                reimbursable_at TEXT,
                reimbursed_at   TEXT,
                reimbursed_amount TEXT NOT NULL DEFAULT '0',
                CHECK (
                    (kind IN ('expense', 'income') AND category_id IS NOT NULL
                     AND to_account_id IS NULL AND target_amount IS NULL)
                    OR
                    (kind = 'transfer' AND category_id IS NULL
                     AND to_account_id IS NOT NULL AND target_amount IS NOT NULL)
                    OR
                    (kind IN ('loan', 'adjustment') AND category_id IS NULL
                     AND to_account_id IS NULL AND target_amount IS NULL)
                )
            );

            CREATE INDEX IF NOT EXISTS idx_transactions_month
                ON transactions(occurred_at, voided_at);
            CREATE INDEX IF NOT EXISTS idx_transactions_account
                ON transactions(account_id, to_account_id);

            CREATE TABLE IF NOT EXISTS auth_sessions (
                token_hash TEXT PRIMARY KEY,
                username   TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_auth_sessions_expiry
                ON auth_sessions(expires_at);
            "#,
        )?;
        if !table_has_column(&conn, "transactions", "currency")? {
            conn.execute("ALTER TABLE transactions ADD COLUMN currency TEXT", [])?;
        }
        if !table_has_column(&conn, "categories", "archived_at")? {
            conn.execute("ALTER TABLE categories ADD COLUMN archived_at TEXT", [])?;
        }
        if !table_has_column(&conn, "transactions", "target_currency")? {
            conn.execute(
                "ALTER TABLE transactions ADD COLUMN target_currency TEXT",
                [],
            )?;
        }
        if !table_has_column(&conn, "transactions", "settled_amount")? {
            conn.execute(
                "ALTER TABLE transactions ADD COLUMN settled_amount TEXT",
                [],
            )?;
        }
        // —— 账户模型扩展（定期/报销/借款）的幂等迁移 ——
        if !table_has_column(&conn, "accounts", "interest_rate")? {
            conn.execute("ALTER TABLE accounts ADD COLUMN interest_rate TEXT", [])?;
        }
        if !table_has_column(&conn, "accounts", "maturity_at")? {
            conn.execute("ALTER TABLE accounts ADD COLUMN maturity_at TEXT", [])?;
        }
        if !table_has_column(&conn, "accounts", "credit_limit")? {
            conn.execute("ALTER TABLE accounts ADD COLUMN credit_limit TEXT", [])?;
        }
        if !table_has_column(&conn, "transactions", "loan_id")? {
            conn.execute("ALTER TABLE transactions ADD COLUMN loan_id INTEGER", [])?;
        }
        if !table_has_column(&conn, "transactions", "reimbursable_at")? {
            conn.execute(
                "ALTER TABLE transactions ADD COLUMN reimbursable_at TEXT",
                [],
            )?;
        }
        if !table_has_column(&conn, "transactions", "reimbursed_at")? {
            conn.execute("ALTER TABLE transactions ADD COLUMN reimbursed_at TEXT", [])?;
        }
        if !table_has_column(&conn, "transactions", "reimbursed_amount")? {
            conn.execute(
                "ALTER TABLE transactions ADD COLUMN reimbursed_amount TEXT NOT NULL DEFAULT '0'",
                [],
            )?;
        }
        // SQLite 无法修改 CHECK 约束：旧表按 asset/liability 建模，需要整表重建。
        // 检测依据：表定义中缺少新类型标记（'cash'/'loan'），无论是否有旧 CHECK。
        if !table_sql_contains(&conn, "accounts", "'cash'")? {
            rebuild_accounts_table(&conn)?;
        }
        if !table_sql_contains(&conn, "transactions", "'adjustment'")? {
            rebuild_transactions_table(&conn)?;
        }
        conn.execute_batch(
            r#"
            UPDATE transactions
            SET currency = (
                SELECT currency FROM accounts WHERE accounts.id = transactions.account_id
            )
            WHERE currency IS NULL;

            UPDATE transactions
            SET target_currency = (
                SELECT currency FROM accounts WHERE accounts.id = transactions.to_account_id
            )
            WHERE kind = 'transfer' AND target_currency IS NULL;

            UPDATE transactions
            SET settled_amount = amount
            WHERE settled_amount IS NULL;

            CREATE INDEX IF NOT EXISTS idx_transactions_currency
                ON transactions(currency, occurred_at, voided_at);
            "#,
        )?;
        Ok(Self { conn })
    }

    pub fn create_auth_session(&mut self, username: &str, ttl_seconds: i64) -> Result<String> {
        let token = generate_session_token()?;
        let now = Utc::now();
        let expires_at = now.timestamp() + ttl_seconds;
        let transaction = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM auth_sessions WHERE expires_at <= ?1",
            [now.timestamp()],
        )?;
        transaction.execute(
            "INSERT INTO auth_sessions(token_hash, username, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![session_token_hash(&token), username, timestamp(now), expires_at],
        )?;
        transaction.commit()?;
        Ok(token)
    }

    pub fn authenticated_username(&self, token: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT username FROM auth_sessions WHERE token_hash = ?1 AND expires_at > ?2",
                params![session_token_hash(token), Utc::now().timestamp()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(KokuError::from)
    }

    pub fn delete_auth_session(&mut self, token: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM auth_sessions WHERE token_hash = ?1",
            [session_token_hash(token)],
        )?;
        Ok(())
    }

    fn account_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Account> {
        let row = tx
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

    fn category_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Category> {
        let row = tx
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

    fn transaction_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Transaction> {
        let raw = tx
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

    fn loan_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Loan> {
        let row = tx
            .query_row(
                "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at FROM loans WHERE id = ?1",
                [id],
                loan_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound { entity: "loan", id })?;
        loan_from_row(row)
    }

    fn set_balance(tx: &SqlTransaction<'_>, account_id: i64, balance: Decimal) -> Result<()> {
        let changed = tx.execute(
            "UPDATE accounts SET balance = ?1 WHERE id = ?2",
            params![decimal_to_db(balance), account_id],
        )?;
        if changed != 1 {
            return Err(KokuError::NotFound {
                entity: "account",
                id: account_id,
            });
        }
        Ok(())
    }
}

type AccountRow = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);
type CategoryRow = (i64, String, String);
type TransactionRow = (
    i64,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
);

fn account_from_row(row: AccountRow) -> Result<Account> {
    Ok(Account {
        id: row.0,
        name: row.1,
        account_type: AccountType::from_db(&row.2)?,
        currency: row.3,
        balance: decimal_from_db(&row.4)?,
        interest_rate: row.5.as_deref().map(decimal_from_db).transpose()?,
        maturity_at: row.6.as_deref().map(parse_timestamp).transpose()?,
        credit_limit: row.7.as_deref().map(decimal_from_db).transpose()?,
    })
}

fn category_from_row(row: CategoryRow) -> Result<Category> {
    Ok(Category {
        id: row.0,
        name: row.1,
        kind: CategoryKind::from_db(&row.2)?,
    })
}

fn transaction_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRow> {
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
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
    ))
}

fn transaction_from_row(row: TransactionRow) -> Result<Transaction> {
    Ok(Transaction {
        id: row.0,
        kind: TransactionKind::from_db(&row.1)?,
        account_id: row.2,
        to_account_id: row.3,
        category_id: row.4,
        amount: decimal_from_db(&row.5)?,
        currency: row.6,
        settled_amount: decimal_from_db(&row.7)?,
        target_amount: row.8.as_deref().map(decimal_from_db).transpose()?,
        target_currency: row.9,
        occurred_at: parse_timestamp(&row.10)?,
        note: row.11,
        voided_at: row.12.as_deref().map(parse_timestamp).transpose()?,
        loan_id: row.13,
        reimbursable_at: row.14.as_deref().map(parse_timestamp).transpose()?,
        reimbursed_at: row.15.as_deref().map(parse_timestamp).transpose()?,
        reimbursed_amount: decimal_from_db(&row.16)?,
    })
}

type LoanRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
);

fn loan_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LoanRow> {
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

fn loan_from_row(row: LoanRow) -> Result<Loan> {
    Ok(Loan {
        id: row.0,
        loan_type: LoanType::from_db(&row.1)?,
        counterparty: row.2,
        currency: row.3,
        principal: decimal_from_db(&row.4)?,
        outstanding: decimal_from_db(&row.5)?,
        account_id: row.6,
        opened_at: parse_timestamp(&row.7)?,
        note: row.8,
        closed_at: row.9.as_deref().map(parse_timestamp).transpose()?,
    })
}

fn required_text(value: String, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(KokuError::InvalidInput(format!("{field} cannot be empty")));
    }
    Ok(trimmed.to_owned())
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in columns {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 检查表的原始 CREATE 语句是否包含指定片段（用于检测旧版 CHECK 约束）。
fn table_sql_contains(conn: &Connection, table: &str, needle: &str) -> Result<bool> {
    let sql = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
    Ok(sql.contains(needle))
}

/// 重建 accounts 表：把旧的 asset/liability CHECK 换成四种新类型，并迁移旧值。
fn rebuild_accounts_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = OFF;
        CREATE TABLE accounts_new (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL UNIQUE,
            account_type  TEXT NOT NULL CHECK (account_type IN ('cash', 'credit', 'savings', 'stock')),
            currency      TEXT NOT NULL,
            balance       TEXT NOT NULL,
            created_at    TEXT NOT NULL,
            interest_rate TEXT,
            maturity_at   TEXT,
            credit_limit  TEXT
        );
        INSERT INTO accounts_new(id, name, account_type, currency, balance, created_at, interest_rate, maturity_at, credit_limit)
            SELECT id, name,
                   CASE account_type
                       WHEN 'asset' THEN 'cash'
                       WHEN 'liability' THEN 'credit'
                       ELSE account_type
                   END,
                   currency, balance, created_at, interest_rate, maturity_at, credit_limit
            FROM accounts;
        DROP TABLE accounts;
        ALTER TABLE accounts_new RENAME TO accounts;
        PRAGMA foreign_keys = ON;
        "#,
    )?;
    Ok(())
}

/// 重建 transactions 表：扩展 kind CHECK（允许 loan）并带上报销/借款新列。
fn rebuild_transactions_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = OFF;
        CREATE TABLE transactions_new (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'loan', 'adjustment')),
            account_id    INTEGER NOT NULL REFERENCES accounts(id),
            to_account_id INTEGER REFERENCES accounts(id),
            category_id   INTEGER REFERENCES categories(id),
            amount        TEXT NOT NULL,
            currency      TEXT,
            settled_amount TEXT,
            target_amount TEXT,
            target_currency TEXT,
            occurred_at   TEXT NOT NULL,
            note          TEXT NOT NULL DEFAULT '',
            voided_at     TEXT,
            loan_id       INTEGER REFERENCES loans(id),
            reimbursable_at TEXT,
            reimbursed_at   TEXT,
            reimbursed_amount TEXT NOT NULL DEFAULT '0',
            CHECK (
                (kind IN ('expense', 'income') AND category_id IS NOT NULL
                 AND to_account_id IS NULL AND target_amount IS NULL)
                OR
                (kind = 'transfer' AND category_id IS NULL
                 AND to_account_id IS NOT NULL AND target_amount IS NOT NULL)
                OR
                (kind IN ('loan', 'adjustment') AND category_id IS NULL
                 AND to_account_id IS NULL AND target_amount IS NULL)
            )
        );
        INSERT INTO transactions_new(id, kind, account_id, to_account_id, category_id, amount,
                                     currency, settled_amount, target_amount, target_currency,
                                     occurred_at, note, voided_at, loan_id,
                                     reimbursable_at, reimbursed_at, reimbursed_amount)
            SELECT id, kind, account_id, to_account_id, category_id, amount,
                   currency, settled_amount, target_amount, target_currency,
                   occurred_at, note, voided_at, loan_id,
                   reimbursable_at, reimbursed_at, COALESCE(reimbursed_amount, '0')
            FROM transactions;
        DROP TABLE transactions;
        ALTER TABLE transactions_new RENAME TO transactions;
        PRAGMA foreign_keys = ON;
        "#,
    )?;
    Ok(())
}

fn normalize_currency(value: String) -> Result<String> {
    let currency = required_text(value, "currency")?.to_uppercase();
    if currency.len() != 3 || !currency.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(KokuError::InvalidInput(
            "currency must be a three-letter ISO-style code".to_owned(),
        ));
    }
    Ok(currency)
}

fn month_bounds(year: i32, month: u32) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let start_date = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| KokuError::InvalidInput(format!("invalid year/month: {year}-{month}")))?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let end_date = NaiveDate::from_ymd_opt(next_year, next_month, 1).ok_or_else(|| {
        KokuError::InvalidInput(format!("invalid next month after {year}-{month}"))
    })?;
    let start = start_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| KokuError::InvalidInput("invalid month start".to_owned()))?
        .and_utc();
    let end = end_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| KokuError::InvalidInput("invalid month end".to_owned()))?
        .and_utc();
    Ok((start, end))
}

fn cash_flow_items(totals: BTreeMap<(i64, String), Decimal>, total: Decimal) -> Vec<CashFlowItem> {
    let hundred = Decimal::from(100_u32);
    let mut items: Vec<_> = totals
        .into_iter()
        .map(|((category_id, category_name), amount)| CashFlowItem {
            category_id,
            category_name,
            amount,
            percentage: if total.is_zero() {
                Decimal::ZERO
            } else {
                (amount / total * hundred).round_dp(2)
            },
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.amount));
    items
}

fn positive_amount(amount: Decimal) -> Result<()> {
    if amount <= Decimal::ZERO {
        return Err(KokuError::InvalidInput(
            "transaction amount must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn decimal_to_db(value: Decimal) -> String {
    value.normalize().to_string()
}

fn decimal_from_db(value: &str) -> Result<Decimal> {
    Ok(Decimal::from_str(value)?)
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| KokuError::InvalidInput(format!("invalid timestamp in database: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DEFAULT_CATEGORIES;
    use chrono::{Datelike, Duration as ChronoDuration};

    fn test_service() -> Result<BookkeepingService> {
        BookkeepingService::in_memory()
    }

    #[test]
    fn server_sessions_are_hashed_expirable_and_revocable() -> Result<()> {
        let mut service = test_service()?;
        let token = service.create_auth_session("somnus", 3600)?;
        assert_eq!(token.len(), 64);
        assert_eq!(
            service.authenticated_username(&token)?.as_deref(),
            Some("somnus")
        );
        let stored_token =
            service
                .conn
                .query_row("SELECT token_hash FROM auth_sessions", [], |row| {
                    row.get::<_, String>(0)
                })?;
        assert_ne!(stored_token, token);

        service.conn.execute(
            "UPDATE auth_sessions SET expires_at = ?1",
            [Utc::now().timestamp() - 1],
        )?;
        assert_eq!(service.authenticated_username(&token)?, None);

        let second_token = service.create_auth_session("somnus", 3600)?;
        service.delete_auth_session(&second_token)?;
        assert_eq!(service.authenticated_username(&second_token)?, None);
        Ok(())
    }

    #[test]
    fn default_categories_are_rich_and_idempotent() -> Result<()> {
        let mut service = test_service()?;
        service.ensure_default_categories()?;
        service.ensure_default_categories()?;

        let categories = service.categories()?;
        assert_eq!(categories.len(), DEFAULT_CATEGORIES.len());
        assert!(categories
            .iter()
            .any(|item| { item.name == "奖金" && item.kind == CategoryKind::Income }));
        assert!(categories
            .iter()
            .any(|item| { item.name == "医疗保健" && item.kind == CategoryKind::Expense }));
        assert!(categories
            .iter()
            .any(|item| { item.name == "其他支出" && item.kind == CategoryKind::Expense }));
        Ok(())
    }

    #[test]
    fn deleted_category_stays_hidden_and_can_be_restored() -> Result<()> {
        let mut service = test_service()?;
        service.ensure_default_categories()?;
        let travel = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "旅行" && item.kind == CategoryKind::Expense)
            .ok_or_else(|| KokuError::InvalidInput("旅行 category is missing".to_owned()))?;

        assert_eq!(service.delete_category(travel.id)?, travel);
        assert!(matches!(
            service.category(travel.id),
            Err(KokuError::NotFound {
                entity: "category",
                id
            }) if id == travel.id
        ));

        service.ensure_default_categories()?;
        assert!(!service
            .categories()?
            .iter()
            .any(|item| item.id == travel.id));

        let restored = service.create_category("旅行", CategoryKind::Expense)?;
        assert_eq!(restored.id, travel.id);
        assert!(service
            .categories()?
            .iter()
            .any(|item| item.id == travel.id));
        Ok(())
    }

    #[test]
    fn deleting_category_preserves_historical_transactions_and_statistics() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("Cash", AccountType::Cash, "CNY", Decimal::from(100_u32))?;
        let food = service.create_category("Food", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let transaction = service.record_expense(account.id, food.id, Decimal::TEN, at, "Lunch")?;

        service.delete_category(food.id)?;

        assert!(!service.categories()?.iter().any(|item| item.id == food.id));
        assert_eq!(
            service.transaction(transaction.id)?.category_id,
            Some(food.id)
        );
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.total_expense, Decimal::TEN);
        assert_eq!(summary.expenses_by_category[0].category_name, "Food");
        assert_eq!(service.account(account.id)?.balance, Decimal::from(90_u32));
        Ok(())
    }

    #[test]
    fn foreign_currency_transaction_updates_one_shared_settlement_balance() -> Result<()> {
        let mut service = test_service()?;
        let visa = service.create_account(
            "CMB Visa",
            AccountType::Credit,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let shopping = service.create_category("Shopping", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();

        let purchase = service.record_expense_in_currency(
            visa.id,
            shopping.id,
            Decimal::from_str("32.50")?,
            "USD",
            Decimal::from_str("234.56")?,
            at,
            "USD purchase",
        )?;

        let account = service.account(visa.id)?;
        assert_eq!(account.currency, "CNY");
        assert_eq!(account.balance, Decimal::from_str("1234.56")?);

        let summary = service.monthly_summary(2026, 8, "USD")?;
        assert_eq!(summary.total_income, Decimal::ZERO);
        assert_eq!(summary.total_expense, Decimal::from_str("32.50")?);
        assert_eq!(purchase.currency, "USD");
        assert_eq!(purchase.settled_amount, Decimal::from_str("234.56")?);
        assert_eq!(
            service.monthly_summary(2026, 8, "CNY")?.total_expense,
            Decimal::ZERO
        );

        service.void_transaction(purchase.id)?;
        let account = service.account(visa.id)?;
        assert_eq!(account.balance, Decimal::from(1000_u32));
        Ok(())
    }

    #[test]
    fn paying_a_liability_reduces_debt_and_void_restores_it() -> Result<()> {
        let mut service = test_service()?;
        let checking = service.create_account(
            "Checking",
            AccountType::Cash,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let visa = service.create_account(
            "Visa debt",
            AccountType::Credit,
            "CNY",
            Decimal::from(500_u32),
        )?;

        let payment = service.record_transfer(
            checking.id,
            visa.id,
            Decimal::from(200_u32),
            Decimal::from(200_u32),
            Utc::now(),
            "card payment",
        )?;
        assert_eq!(
            service.account(checking.id)?.balance,
            Decimal::from(800_u32)
        );
        assert_eq!(service.account(visa.id)?.balance, Decimal::from(300_u32));

        service.void_transaction(payment.id)?;
        assert_eq!(
            service.account(checking.id)?.balance,
            Decimal::from(1000_u32)
        );
        assert_eq!(service.account(visa.id)?.balance, Decimal::from(500_u32));
        Ok(())
    }

    #[test]
    fn legacy_single_currency_database_is_migrated_without_losing_balances() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            r#"
            CREATE TABLE accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                account_type TEXT NOT NULL,
                currency TEXT NOT NULL,
                balance TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE categories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(name, kind)
            );
            CREATE TABLE transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                account_id INTEGER NOT NULL REFERENCES accounts(id),
                to_account_id INTEGER REFERENCES accounts(id),
                category_id INTEGER REFERENCES categories(id),
                amount TEXT NOT NULL,
                target_amount TEXT,
                occurred_at TEXT NOT NULL,
                note TEXT NOT NULL DEFAULT '',
                voided_at TEXT
            );
            INSERT INTO accounts(name, account_type, currency, balance, created_at)
                VALUES ('Legacy Visa', 'liability', 'CNY', '456.78', '2026-08-15T00:00:00Z');
            INSERT INTO categories(name, kind, created_at)
                VALUES ('Legacy Shopping', 'expense', '2026-08-15T00:00:00Z');
            INSERT INTO transactions(kind, account_id, category_id, amount, occurred_at, note)
                VALUES ('expense', 1, 1, '12.34', '2026-08-15T00:00:00Z', 'legacy');
            "#,
        )?;

        let service = BookkeepingService::from_connection(conn)?;
        let account = service.account(1)?;
        assert_eq!(account.currency, "CNY");
        assert_eq!(account.balance, Decimal::from_str("456.78")?);
        let transaction = service.transaction(1)?;
        assert_eq!(transaction.currency, "CNY");
        assert_eq!(transaction.settled_amount, Decimal::from_str("12.34")?);
        Ok(())
    }

    #[test]
    fn transfer_updates_both_balances_exactly_and_is_atomic() -> Result<()> {
        let mut service = test_service()?;
        let source = service.create_account(
            "Source",
            AccountType::Cash,
            "CNY",
            Decimal::from_str("1000.10")?,
        )?;
        let target = service.create_account(
            "Target",
            AccountType::Cash,
            "CNY",
            Decimal::from_str("10.20")?,
        )?;

        service.record_transfer(
            source.id,
            target.id,
            Decimal::from_str("333.33")?,
            Decimal::from_str("333.33")?,
            Utc::now(),
            "exact transfer",
        )?;
        assert_eq!(
            service.account(source.id)?.balance,
            Decimal::from_str("666.77")?
        );
        assert_eq!(
            service.account(target.id)?.balance,
            Decimal::from_str("343.53")?
        );

        let before = service.account(source.id)?.balance;
        let result = service.record_transfer(
            source.id,
            999_999,
            Decimal::ONE,
            Decimal::ONE,
            Utc::now(),
            "must roll back",
        );
        assert!(matches!(result, Err(KokuError::NotFound { .. })));
        assert_eq!(service.account(source.id)?.balance, before);
        Ok(())
    }

    #[test]
    fn void_transfer_restores_both_accounts_once() -> Result<()> {
        let mut service = test_service()?;
        let source =
            service.create_account("Wallet", AccountType::Cash, "CNY", Decimal::from(500_u32))?;
        let target =
            service.create_account("Bank", AccountType::Cash, "CNY", Decimal::from(100_u32))?;
        let transfer = service.record_transfer(
            source.id,
            target.id,
            Decimal::from_str("88.88")?,
            Decimal::from_str("88.88")?,
            Utc::now(),
            "temporary",
        )?;

        let voided = service.void_transaction(transfer.id)?;
        assert!(voided.voided_at.is_some());
        assert_eq!(service.account(source.id)?.balance, Decimal::from(500_u32));
        assert_eq!(service.account(target.id)?.balance, Decimal::from(100_u32));
        assert!(matches!(
            service.void_transaction(transfer.id),
            Err(KokuError::AlreadyVoided)
        ));
        assert_eq!(service.account(source.id)?.balance, Decimal::from(500_u32));
        Ok(())
    }

    #[test]
    fn categorized_transactions_and_void_are_reflected_in_monthly_stats() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("Checking", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let salary = service.create_category("Salary", CategoryKind::Income)?;
        let food = service.create_category("Food", CategoryKind::Expense)?;
        let transit = service.create_category("Transit", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();

        service.record_income(account.id, salary.id, Decimal::from(1000_u32), at, "salary")?;
        service.record_expense(account.id, food.id, Decimal::from(150_u32), at, "food")?;
        service.record_expense(account.id, transit.id, Decimal::from(50_u32), at, "transit")?;
        let mistake =
            service.record_expense(account.id, food.id, Decimal::from(25_u32), at, "mistake")?;
        service.void_transaction(mistake.id)?;

        let summary = service.monthly_summary(2026, 8, "cny")?;
        assert_eq!(summary.total_income, Decimal::from(1000_u32));
        assert_eq!(summary.total_expense, Decimal::from(200_u32));
        assert_eq!(summary.net, Decimal::from(800_u32));
        assert_eq!(summary.expenses_by_category.len(), 2);
        assert_eq!(summary.expenses_by_category[0].category_name, "Food");
        assert_eq!(
            summary.expenses_by_category[0].percentage,
            Decimal::from(75_u32)
        );
        assert_eq!(service.account(account.id)?.balance, Decimal::from(800_u32));

        let cash_flow = service.cash_flow_summary(2026, 8, "CNY")?;
        assert_eq!(cash_flow.flow_total, Decimal::from(1000_u32));
        assert_eq!(cash_flow.retained, Decimal::from(800_u32));
        assert_eq!(cash_flow.income_sources.len(), 1);
        assert_eq!(cash_flow.income_sources[0].category_name, "Salary");
        assert_eq!(
            cash_flow.income_sources[0].percentage,
            Decimal::from(100_u32)
        );
        assert_eq!(cash_flow.expense_destinations.len(), 2);
        assert_eq!(cash_flow.expense_destinations[0].category_name, "Food");
        Ok(())
    }

    #[test]
    fn cross_currency_transfer_uses_explicit_target_amounts() -> Result<()> {
        let mut service = test_service()?;
        let cny = service.create_account(
            "CNY account",
            AccountType::Cash,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let usd = service.create_account("USD account", AccountType::Cash, "USD", Decimal::ZERO)?;
        service.record_transfer(
            cny.id,
            usd.id,
            Decimal::from(720_u32),
            Decimal::from(100_u32),
            Utc::now(),
            "explicit FX conversion",
        )?;
        assert_eq!(service.account(cny.id)?.balance, Decimal::from(280_u32));
        assert_eq!(service.account(usd.id)?.balance, Decimal::from(100_u32));
        Ok(())
    }

    #[test]
    fn transactions_are_paginated_newest_first_and_limited() -> Result<()> {
        let mut service = test_service()?;
        let account = service.create_account("Cash", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("Food", CategoryKind::Expense)?;
        let at = |day: u32| {
            NaiveDate::from_ymd_opt(2026, 8, day)
                .and_then(|date| date.and_hms_opt(12, 0, 0))
                .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))
                .map(|date| date.and_utc())
        };
        service.record_expense(account.id, food.id, Decimal::TEN, at(1)?, "first")?;
        service.record_expense(account.id, food.id, Decimal::from(20_u32), at(2)?, "second")?;
        service.record_expense(account.id, food.id, Decimal::from(30_u32), at(3)?, "third")?;

        let page1 = service.transactions(2, 0)?;
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].note, "third");
        assert_eq!(page1[1].note, "second");

        let page2 = service.transactions(2, 2)?;
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].note, "first");

        assert!(matches!(
            service.transactions(0, 0),
            Err(KokuError::InvalidInput(_))
        ));
        assert!(matches!(
            service.transactions(1001, 0),
            Err(KokuError::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn fixed_deposit_earns_interest_and_settles_back_to_savings() -> Result<()> {
        let mut service = test_service()?;
        let savings = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let deposit = service.create_fixed_deposit(
            savings.id,
            Decimal::from(5000_u32),
            "CNY",
            Decimal::from_str("2.10")?,
            365,
            "一年定期",
        )?;
        assert_eq!(deposit.account_type, AccountType::Savings);
        assert_eq!(deposit.interest_rate, Some(Decimal::from_str("2.10")?));
        assert!(deposit.maturity_at.is_some());
        assert_eq!(
            service.account(savings.id)?.balance,
            Decimal::from(5000_u32)
        );
        assert_eq!(deposit.balance, Decimal::from(5000_u32));

        // 把起存日期拨回 100 天，制造利息
        let start = Utc::now() - ChronoDuration::days(100);
        service.conn.execute(
            "UPDATE accounts SET created_at = ?1 WHERE id = ?2",
            params![timestamp(start), deposit.id],
        )?;
        let settlement = service.settle_deposit(deposit.id, savings.id)?;
        // 5000 * 2.10% * 100/365 ≈ 28.77
        assert_eq!(settlement.interest, Decimal::from_str("28.77")?);
        assert_eq!(settlement.transfer.kind, TransactionKind::Transfer);
        assert_eq!(
            service.account(savings.id)?.balance,
            Decimal::from_str("10028.77")?
        );
        assert_eq!(service.account(deposit.id)?.balance, Decimal::ZERO);

        // 已结清的定期不能再结
        assert!(matches!(
            service.settle_deposit(deposit.id, savings.id),
            Err(KokuError::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn reimbursement_marks_settles_partially_and_drops_from_expenses() -> Result<()> {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "出差餐费")?;

        // 未标记直接报销 -> 拒绝
        assert!(matches!(
            service.reimburse(expense.id, cash.id, Decimal::TEN, "CNY", None, ""),
            Err(KokuError::InvalidInput(_))
        ));
        let marked = service.mark_reimbursable(expense.id)?;
        assert!(marked.reimbursable_at.is_some());

        // 部分报销 40
        let income1 = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(40_u32),
            "CNY",
            None,
            "报销首笔",
        )?;
        assert_eq!(income1.kind, TransactionKind::Income);
        assert_eq!(
            income1.category_id,
            Some(
                service
                    .categories()?
                    .iter()
                    .find(|c| c.name == "报销")
                    .unwrap()
                    .id
            )
        );
        let expense_after = service.transaction(expense.id)?;
        assert_eq!(expense_after.reimbursed_amount, Decimal::from(40_u32));
        assert!(expense_after.reimbursed_at.is_none());
        // 支出扣了 100，报销回 40
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(-60_i32));

        // 超额报销 -> 拒绝
        assert!(matches!(
            service.reimburse(expense.id, cash.id, Decimal::from(61_u32), "CNY", None, ""),
            Err(KokuError::InvalidInput(_))
        ));
        // 报完剩余 60
        service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(60_u32),
            "CNY",
            None,
            "报销尾款",
        )?;
        let expense_done = service.transaction(expense.id)?;
        assert_eq!(expense_done.reimbursed_amount, Decimal::from(100_u32));
        assert!(expense_done.reimbursed_at.is_some());

        // 已报销金额从月度支出剔除，报销收入计入收入
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.total_expense, Decimal::ZERO);
        assert_eq!(summary.total_income, Decimal::from(100_u32));
        assert_eq!(service.account(cash.id)?.balance, Decimal::ZERO);
        Ok(())
    }

    #[test]
    fn account_can_be_edited_and_balance_adjusted_with_audit_trail() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(100_u32))?;

        // 改名/改类型
        let updated = service.update_account(
            account.id,
            Some("微信零钱".to_owned()),
            Some(AccountType::Cash),
            None,
        )?;
        assert_eq!(updated.name, "微信零钱");

        // 调整余额 +50，产生一条 adjustment 流水且不计入收支统计
        let adjustment = service.adjust_balance(account.id, Decimal::from(50_u32), "补记现金")?;
        assert_eq!(adjustment.kind, TransactionKind::Adjustment);
        assert_eq!(adjustment.amount, Decimal::from(50_u32));
        assert_eq!(service.account(account.id)?.balance, Decimal::from(150_u32));
        let summary = service.monthly_summary(Utc::now().year(), Utc::now().month(), "CNY")?;
        assert_eq!(summary.total_income, Decimal::ZERO);
        assert_eq!(summary.total_expense, Decimal::ZERO);

        // 撤销调整 → 余额恢复
        service.void_transaction(adjustment.id)?;
        assert_eq!(service.account(account.id)?.balance, Decimal::from(100_u32));

        // 已有交易历史时不允许改币种
        assert!(matches!(
            service.update_account(account.id, None, None, Some("USD".to_owned())),
            Err(KokuError::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn credit_limit_can_be_set_cleared_and_reimbursable_unmarked() -> Result<()> {
        let mut service = test_service()?;
        let credit = service.create_account("信用卡", AccountType::Credit, "CNY", Decimal::ZERO)?;

        // 设置额度
        let with_limit = service.set_credit_limit(credit.id, Some(Decimal::from(20000_u32)))?;
        assert_eq!(with_limit.credit_limit, Some(Decimal::from(20000_u32)));
        // 清除额度
        assert_eq!(
            service.set_credit_limit(credit.id, None)?.credit_limit,
            None
        );

        // 报销标记 → 取消 → 再标记 → 报销后不可取消
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "餐费")?;

        service.mark_reimbursable(expense.id)?;
        assert!(service.transaction(expense.id)?.reimbursable_at.is_some());
        let unmarked = service.unmark_reimbursable(expense.id)?;
        assert!(unmarked.reimbursable_at.is_none());
        assert!(matches!(
            service.unmark_reimbursable(expense.id),
            Err(KokuError::InvalidInput(_))
        ));

        service.mark_reimbursable(expense.id)?;
        service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(100_u32),
            "CNY",
            None,
            "报销",
        )?;
        assert!(matches!(
            service.unmark_reimbursable(expense.id),
            Err(KokuError::InvalidInput(_))
        ));
        Ok(())
    }

    #[test]
    fn loans_from_same_counterparty_merge_until_settled() -> Result<()> {
        let mut service = test_service()?;
        let savings = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;

        // 两次借出给张三 → 合并为一条，本金/未结 1500
        let first = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(1000_u32),
            savings.id,
            "",
        )?;
        let second = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(500_u32),
            savings.id,
            "再借",
        )?;
        assert_eq!(second.id, first.id);
        assert_eq!(second.principal, Decimal::from(1500_u32));
        assert_eq!(second.outstanding, Decimal::from(1500_u32));
        assert_eq!(service.loans()?.len(), 1);

        // 部分还款后再次借出：principal 累加，outstanding 只加新本金
        service.repay_loan(
            first.id,
            savings.id,
            Decimal::from(400_u32),
            "CNY",
            None,
            "",
        )?;
        let third = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(200_u32),
            savings.id,
            "",
        )?;
        assert_eq!(third.id, first.id);
        assert_eq!(third.principal, Decimal::from(1700_u32));
        assert_eq!(third.outstanding, Decimal::from(1300_u32));

        // 不同方向（借入）、不同人是独立记录
        service.create_loan(
            LoanType::Borrow,
            "张三",
            "CNY",
            Decimal::from(300_u32),
            savings.id,
            "",
        )?;
        service.create_loan(
            LoanType::Lend,
            "李四",
            "CNY",
            Decimal::from(100_u32),
            savings.id,
            "",
        )?;
        assert_eq!(service.loans()?.len(), 3);

        // 还清后再次借出 → 另起一条
        service.repay_loan(
            first.id,
            savings.id,
            Decimal::from(1300_u32),
            "CNY",
            None,
            "结清",
        )?;
        assert!(service.loan(first.id)?.closed_at.is_some());
        let fourth = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(50_u32),
            savings.id,
            "",
        )?;
        assert_ne!(fourth.id, first.id);
        assert_eq!(fourth.principal, Decimal::from(50_u32));
        Ok(())
    }

    #[test]
    fn loans_lend_borrow_and_repay_across_accounts() -> Result<()> {
        let mut service = test_service()?;
        let savings = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;

        // 借出 1000 给张三（从储蓄出账）
        let lend = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(1000_u32),
            savings.id,
            "朋友借款",
        )?;
        assert_eq!(lend.outstanding, Decimal::from(1000_u32));
        assert_eq!(
            service.account(savings.id)?.balance,
            Decimal::from(9000_u32)
        );
        // 净资产不受影响：9000 余额 + 1000 应收
        let balance = service.balance_summary("CNY")?;
        assert_eq!(balance.total_assets, Decimal::from(10000_u32));
        assert_eq!(balance.net_worth, Decimal::from(10000_u32));
        // 借出流水不计入收支统计
        assert_eq!(
            service
                .monthly_summary(Utc::now().year(), Utc::now().month(), "CNY")?
                .total_expense,
            Decimal::ZERO
        );
        assert_eq!(
            service
                .transactions(10, 0)?
                .iter()
                .filter(|t| t.loan_id == Some(lend.id))
                .count(),
            1
        );

        // 还款 400 到零钱
        let lend = service.repay_loan(
            lend.id,
            cash.id,
            Decimal::from(400_u32),
            "CNY",
            None,
            "首期还款",
        )?;
        assert_eq!(lend.outstanding, Decimal::from(600_u32));
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(400_u32));

        // 借入 2000（银行），到账零钱
        let borrow = service.create_loan(
            LoanType::Borrow,
            "银行",
            "CNY",
            Decimal::from(2000_u32),
            cash.id,
            "周转",
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(2400_u32));
        let balance = service.balance_summary("CNY")?;
        assert_eq!(balance.total_liabilities, Decimal::from(2000_u32));

        // 超额还款 -> 拒绝
        assert!(matches!(
            service.repay_loan(borrow.id, cash.id, Decimal::from(2001_u32), "CNY", None, ""),
            Err(KokuError::InvalidInput(_))
        ));
        // 借出流水不能撤销
        let lend_tx = service
            .transactions(10, 0)?
            .into_iter()
            .find(|t| t.loan_id == Some(lend.id))
            .unwrap();
        assert!(matches!(
            service.void_transaction(lend_tx.id),
            Err(KokuError::InvalidInput(_))
        ));

        // 还清借出 + 借入
        service.repay_loan(
            lend.id,
            cash.id,
            Decimal::from(600_u32),
            "CNY",
            None,
            "结清",
        )?;
        assert!(service.loan(lend.id)?.closed_at.is_some());
        service.repay_loan(
            borrow.id,
            cash.id,
            Decimal::from(2000_u32),
            "CNY",
            None,
            "还清",
        )?;
        assert!(service.loan(borrow.id)?.closed_at.is_some());
        let balance = service.balance_summary("CNY")?;
        assert_eq!(balance.total_liabilities, Decimal::ZERO);
        Ok(())
    }
}
