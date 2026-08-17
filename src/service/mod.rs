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
mod budgets;
mod holdings;
mod loans;
mod rates;
mod receipts;
mod recurring;
mod reimbursements;
mod summaries;
mod tags;
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

            CREATE TABLE IF NOT EXISTS budgets (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                category_id  INTEGER NOT NULL REFERENCES categories(id),
                year         INTEGER NOT NULL,
                month        INTEGER NOT NULL,
                limit_amount TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                UNIQUE(category_id, year, month)
            );

            CREATE TABLE IF NOT EXISTS recurring_rules (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                kind        TEXT NOT NULL CHECK (kind IN ('expense', 'income')),
                account_id  INTEGER NOT NULL REFERENCES accounts(id),
                category_id INTEGER NOT NULL REFERENCES categories(id),
                amount      TEXT NOT NULL,
                note        TEXT NOT NULL DEFAULT '',
                frequency   TEXT NOT NULL CHECK (frequency IN ('monthly', 'weekly')),
                next_due_at TEXT NOT NULL,
                created_at  TEXT NOT NULL,
                paused_at   TEXT
            );

            CREATE TABLE IF NOT EXISTS receipts (
                transaction_id INTEGER PRIMARY KEY REFERENCES transactions(id),
                content_type   TEXT NOT NULL,
                data           BLOB NOT NULL,
                created_at     TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tags (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transaction_tags (
                transaction_id INTEGER NOT NULL REFERENCES transactions(id),
                tag_id         INTEGER NOT NULL REFERENCES tags(id),
                PRIMARY KEY (transaction_id, tag_id)
            );

            CREATE TABLE IF NOT EXISTS holdings (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id INTEGER NOT NULL REFERENCES accounts(id),
                symbol     TEXT NOT NULL,
                shares     TEXT NOT NULL,
                cost_basis TEXT NOT NULL,
                last_price TEXT,
                updated_at TEXT NOT NULL,
                UNIQUE(account_id, symbol)
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
                closed_at    TEXT,
                due_at       TEXT
            );

            CREATE TABLE IF NOT EXISTS reimbursements (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                expense_id    INTEGER NOT NULL REFERENCES transactions(id),
                income_id     INTEGER NOT NULL REFERENCES transactions(id),
                amount        TEXT NOT NULL,
                reimbursed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS exchange_rates (
                base   TEXT NOT NULL,
                quote  TEXT NOT NULL,
                rate   TEXT NOT NULL,
                date   TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'frankfurter',
                PRIMARY KEY (base, quote, date)
            );

            CREATE TABLE IF NOT EXISTS transactions (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'loan', 'adjustment', 'trade')),
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
                    (kind IN ('loan', 'adjustment', 'trade') AND category_id IS NULL
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

            CREATE TABLE IF NOT EXISTS app_settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
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
        if !table_has_column(&conn, "loans", "due_at")? {
            conn.execute("ALTER TABLE loans ADD COLUMN due_at TEXT", [])?;
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
        if !table_sql_contains(&conn, "transactions", "'trade'")? {
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

    /// 作废所有登录会话（改密码后强制重新登录）。
    pub fn delete_all_auth_sessions(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM auth_sessions", [])?;
        Ok(())
    }

    /// 读取持久化的应用设置；不存在时返回 None。
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(KokuError::from)
    }

    /// 写入（覆盖）一条应用设置。
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
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
                "SELECT id, kind, account_id, to_account_id, category_id, amount, currency, settled_amount, target_amount, target_currency, occurred_at, note, voided_at, loan_id, reimbursable_at, reimbursed_at, reimbursed_amount, EXISTS(SELECT 1 FROM receipts r WHERE r.transaction_id = transactions.id) AS has_receipt, COALESCE((SELECT group_concat(t.name, ',') FROM tags t JOIN transaction_tags tt ON tt.tag_id = t.id WHERE tt.transaction_id = transactions.id), '') AS tags FROM transactions WHERE id = ?1",
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
                "SELECT id, loan_type, counterparty, currency, principal, outstanding, account_id, opened_at, note, closed_at, due_at FROM loans WHERE id = ?1",
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

    /// 某笔支出的全部报销关联：(income_id, 报销金额)。
    fn reimbursements_for_expense_in_tx(
        tx: &SqlTransaction<'_>,
        expense_id: i64,
    ) -> Result<Vec<(i64, Decimal)>> {
        let mut statement = tx.prepare(
            "SELECT income_id, amount FROM reimbursements WHERE expense_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([expense_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut links = Vec::new();
        for row in rows {
            let (income_id, amount) = row?;
            links.push((income_id, decimal_from_db(&amount)?));
        }
        Ok(links)
    }

    /// 若某笔收入是报销收入，返回其关联的 (expense_id, 报销金额)。
    fn reimbursement_for_income_in_tx(
        tx: &SqlTransaction<'_>,
        income_id: i64,
    ) -> Result<Option<(i64, Decimal)>> {
        let Some((expense_id, amount)) = tx
            .query_row(
                "SELECT expense_id, amount FROM reimbursements WHERE income_id = ?1",
                [income_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        Ok(Some((expense_id, decimal_from_db(&amount)?)))
    }

    /// 撤销一笔报销收入流水：余额反向恢复并置 voided_at；已撤销则跳过。
    fn void_reimbursement_income_in_tx(tx: &SqlTransaction<'_>, income_id: i64) -> Result<()> {
        let income = Self::transaction_in_tx(tx, income_id)?;
        if income.voided_at.is_some() {
            return Ok(());
        }
        let source = Self::account_in_tx(tx, income.account_id)?;
        Self::set_balance(
            tx,
            income.account_id,
            source
                .account_type
                .apply_outflow(source.balance, income.settled_amount),
        )?;
        tx.execute(
            "UPDATE transactions SET voided_at = ?1 WHERE id = ?2 AND voided_at IS NULL",
            params![timestamp(Utc::now()), income_id],
        )?;
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
    bool,
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
        row.get(17)?,
        row.get(18)?,
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
        has_receipt: row.17,
        tags: split_tags(&row.18),
    })
}

/// 把 group_concat 得到的逗号分隔标签串拆成去空白的标签名列表。
fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
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
        row.get(10)?,
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
        due_at: row.10.as_deref().map(parse_timestamp).transpose()?,
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
            kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'loan', 'adjustment', 'trade')),
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
                (kind IN ('loan', 'adjustment', 'trade') AND category_id IS NULL
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

pub(crate) fn normalize_currency(value: String) -> Result<String> {
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
    use crate::domain::{RateQuote, RecurrenceFrequency, DEFAULT_CATEGORIES};
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
    fn app_settings_round_trip_and_sessions_can_be_invalidated() -> Result<()> {
        let mut service = test_service()?;
        assert_eq!(service.get_setting("password_hash")?, None);
        service.set_setting("password_hash", "hashed")?;
        assert_eq!(
            service.get_setting("password_hash")?.as_deref(),
            Some("hashed")
        );
        service.set_setting("password_hash", "new")?;
        assert_eq!(
            service.get_setting("password_hash")?.as_deref(),
            Some("new")
        );

        let token = service.create_auth_session("somnus", 3600)?;
        assert!(service.authenticated_username(&token)?.is_some());
        service.delete_all_auth_sessions()?;
        assert_eq!(service.authenticated_username(&token)?, None);
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

        // 新语义：汇总按显示币种折算所有币种金额。缓存 USD→CNY 汇率后，
        // 32.50 USD 折算进 CNY 汇总 ≈ 234.56（结算金额）。
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str("7.2172")?,
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;
        let summary_usd = service.monthly_summary(2026, 8, "USD")?;
        assert_eq!(summary_usd.total_income, Decimal::ZERO);
        assert_eq!(summary_usd.total_expense, Decimal::from_str("32.50")?);
        let summary_cny = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(
            summary_cny.total_expense.round_dp(2),
            Decimal::from_str("234.56")?
        );
        assert_eq!(purchase.currency, "USD");
        assert_eq!(purchase.settled_amount, Decimal::from_str("234.56")?);

        service.void_transaction(purchase.id)?;
        let account = service.account(visa.id)?;
        assert_eq!(account.balance, Decimal::from(1000_u32));
        Ok(())
    }

    #[test]
    fn monthly_summary_converts_mixed_currency_transactions_to_the_display_currency() -> Result<()>
    {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let usd = service.create_account("美元储蓄", AccountType::Savings, "USD", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "中餐")?;
        service.record_expense_in_currency(
            usd.id,
            food.id,
            Decimal::from(10_u32),
            "USD",
            Decimal::from(10_u32),
            at,
            "美餐",
        )?;
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from(7_u32),
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;

        // 显示 CNY：100 CNY + 10 USD×7 = 170。
        let cny = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(cny.total_expense, Decimal::from(170_u32));
        // 显示 USD：100 CNY ÷ 7 + 10 USD = 24.29（反向折算）。
        let usd_summary = service.monthly_summary(2026, 8, "USD")?;
        assert_eq!(
            usd_summary.total_expense.round_dp(2),
            Decimal::from_str("24.29")?
        );
        // 缺汇率的币种会报错而不是被静默漏算。
        service.conn.execute("DELETE FROM exchange_rates", [])?;
        assert!(service.monthly_summary(2026, 8, "CNY").is_err());
        Ok(())
    }

    #[test]
    fn transactions_can_be_filtered_by_month_and_paginated() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let salary = service.create_category("工资", CategoryKind::Income)?;
        let august = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let september = NaiveDate::from_ymd_opt(2026, 9, 2)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        service.record_expense(cash.id, food.id, Decimal::from(10_u32), august, "八月餐")?;
        service.record_expense(cash.id, food.id, Decimal::from(20_u32), september, "九月餐")?;
        service.record_income(
            cash.id,
            salary.id,
            Decimal::from(100_u32),
            september,
            "九月工资",
        )?;

        // 按月过滤：八月只有一条，九月有两条。
        let august_txs = service.transactions_in_month(2026, 8, 100, 0)?;
        assert_eq!(august_txs.len(), 1);
        assert_eq!(august_txs[0].note, "八月餐");
        let september_txs = service.transactions_in_month(2026, 9, 100, 0)?;
        assert_eq!(september_txs.len(), 2);

        // 月内分页：limit=1 且 offset=1 返回第二笔（时间倒序、id 倒序）。
        let page = service.transactions_in_month(2026, 9, 1, 1)?;
        assert_eq!(page.len(), 1);
        assert_ne!(page[0].id, september_txs[0].id);

        // 无流水的月份返回空；非法月份报错。
        assert!(service.transactions_in_month(2026, 7, 100, 0)?.is_empty());
        assert!(service.transactions_in_month(2026, 13, 100, 0).is_err());
        Ok(())
    }

    #[test]
    fn monthly_trend_aggregates_recent_months_in_display_currency() -> Result<()> {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let salary = service.create_category("工资", CategoryKind::Income)?;
        let now = Utc::now();
        let previous = now - ChronoDuration::days(40);
        service.record_expense(cash.id, food.id, Decimal::from(250_u32), now, "本月支出")?;
        service.record_income(
            cash.id,
            salary.id,
            Decimal::from(1000_u32),
            previous,
            "上月收入",
        )?;

        let trend = service.monthly_trend(12, "CNY")?;
        assert_eq!(trend.len(), 12);
        // 升序：最后一点是当前月。
        let current = trend.last().unwrap();
        assert_eq!((current.year, current.month), (now.year(), now.month()));
        assert_eq!(current.total_income, Decimal::ZERO);
        assert_eq!(current.total_expense, Decimal::from(250_u32));
        assert_eq!(current.net, Decimal::from(-250_i32));
        // 上一个自然月：只有收入。
        let prev_point = trend
            .iter()
            .find(|point| point.year == previous.year() && point.month == previous.month())
            .ok_or_else(|| {
                KokuError::InvalidInput("previous month missing from trend".to_owned())
            })?;
        assert_eq!(prev_point.total_income, Decimal::from(1000_u32));
        assert_eq!(prev_point.total_expense, Decimal::ZERO);
        assert_eq!(prev_point.net, Decimal::from(1000_u32));
        // 其余月份补零。
        let nonzero = trend
            .iter()
            .filter(|point| !point.total_income.is_zero() || !point.total_expense.is_zero())
            .count();
        assert_eq!(nonzero, 2);
        Ok(())
    }

    #[test]
    fn budgets_round_trip_and_attach_limits_to_monthly_summary() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let salary = service.create_category("工资", CategoryKind::Income)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        service.record_expense(cash.id, food.id, Decimal::from(120_u32), at, "餐费")?;

        // 未设预算时 monthly_summary 不返回 budget_limit。
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.expenses_by_category[0].budget_limit, None);

        // 设置预算并回填。
        let budget = service.set_budget(food.id, 2026, 8, Decimal::from(100_u32))?;
        assert_eq!(budget.limit_amount, Decimal::from(100_u32));
        assert_eq!(budget.category_name, "餐饮");
        assert_eq!(budget.category_kind, CategoryKind::Expense);
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(
            summary.expenses_by_category[0].budget_limit,
            Some(Decimal::from(100_u32))
        );

        // 同月覆盖，列表只有一条。
        service.set_budget(food.id, 2026, 8, Decimal::from(200_u32))?;
        let budgets = service.budgets(2026, 8)?;
        assert_eq!(budgets.len(), 1);
        assert_eq!(budgets[0].limit_amount, Decimal::from(200_u32));

        // 收入分类不能设预算。
        assert!(service
            .set_budget(salary.id, 2026, 8, Decimal::from(50_u32))
            .is_err());

        // 清除后不再回填。
        service.clear_budget(food.id, 2026, 8)?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.expenses_by_category[0].budget_limit, None);
        Ok(())
    }

    #[test]
    fn recurring_rules_generate_due_transactions_and_advance() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let due = Utc::now() - ChronoDuration::days(1);
        let rule = service.create_recurring_rule(
            TransactionKind::Expense,
            cash.id,
            food.id,
            Decimal::from(50_u32),
            "房租".to_owned(),
            RecurrenceFrequency::Monthly,
            due,
        )?;
        assert_eq!(rule.frequency, RecurrenceFrequency::Monthly);

        // 到期生成一笔并推进到下一个周期。
        let generated = service.run_recurring()?;
        assert_eq!(generated.len(), 1);
        assert_eq!(generated[0].note, "房租");
        assert_eq!(generated[0].amount, Decimal::from(50_u32));
        let updated = service.recurring_rule(rule.id)?;
        assert!(updated.next_due_at > due);
        // 已推进，未到期不再生成。
        assert!(service.run_recurring()?.is_empty());

        // 删除规则后无法再查询。
        service.delete_recurring_rule(rule.id)?;
        assert!(matches!(
            service.recurring_rule(rule.id),
            Err(KokuError::NotFound { entity, .. }) if entity == "recurring rule"
        ));
        Ok(())
    }

    #[test]
    fn receipts_attach_and_round_trip_binary_data() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(10_u32), at, "餐费")?;
        assert!(!expense.has_receipt);
        assert!(service.receipt(expense.id).is_err());

        let bytes = vec![0_u8, 1, 2, 255, 254];
        let receipt = service.attach_receipt(expense.id, "image/png".to_owned(), bytes.clone())?;
        assert_eq!(receipt.byte_length, 5);
        assert_eq!(receipt.content_type, "image/png");

        assert!(service.transaction(expense.id)?.has_receipt);
        let (content_type, data) = service.receipt_bytes(expense.id)?;
        assert_eq!(content_type, "image/png");
        assert_eq!(data, bytes);
        Ok(())
    }

    #[test]
    fn receipts_reject_untrusted_content_types() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(10_u32), at, "餐费")?;
        let bytes = vec![1_u8, 2, 3];

        // 白名单内（含大小写与参数）放行并归一化。
        service.attach_receipt(expense.id, "image/jpeg".to_owned(), bytes.clone())?;
        assert_eq!(service.receipt(expense.id)?.content_type, "image/jpeg");
        service.attach_receipt(
            expense.id,
            "Image/PNG; charset=binary".to_owned(),
            bytes.clone(),
        )?;
        assert_eq!(service.receipt(expense.id)?.content_type, "image/png");

        // 可携带脚本的类型与未知类型一律拒绝。
        for bad in [
            "text/html",
            "image/svg+xml",
            "application/octet-stream",
            "text/plain",
        ] {
            assert!(
                service
                    .attach_receipt(expense.id, bad.to_owned(), bytes.clone())
                    .is_err(),
                "should reject {bad}"
            );
        }
        Ok(())
    }

    #[test]
    fn tags_attach_to_transactions_and_round_trip() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(10_u32), at, "餐费")?;
        assert!(expense.tags.is_empty());

        // 设置标签：自动创建 + 回读。
        let tags =
            service.set_transaction_tags(expense.id, vec!["旅行".to_owned(), "出差".to_owned()])?;
        assert_eq!(tags.len(), 2);
        assert!(service.all_tags()?.iter().any(|tag| tag.name == "旅行"));

        let tx = service.transaction(expense.id)?;
        assert_eq!(tx.tags.len(), 2);
        assert!(tx.tags.iter().any(|name| name == "旅行"));

        // 整体替换成单个标签。
        service.set_transaction_tags(expense.id, vec!["报销".to_owned()])?;
        let tx = service.transaction(expense.id)?;
        assert_eq!(tx.tags, vec!["报销".to_owned()]);

        // 非法标签名被拒。
        assert!(service
            .set_transaction_tags(expense.id, vec!["a,b".to_owned()])
            .is_err());
        assert!(service
            .set_transaction_tags(expense.id, vec!["  ".to_owned()])
            .is_err());
        Ok(())
    }

    #[test]
    fn stock_buy_sell_tracks_holdings_and_cash() -> Result<()> {
        let mut service = test_service()?;
        let broker =
            service.create_account("券商", AccountType::Stock, "CNY", Decimal::from(10000_u32))?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();

        // 买入 10 股 @ 150：现金 -1500，持仓 +10。
        let buy = service.buy_stock(
            broker.id,
            "AAPL".to_owned(),
            Decimal::from(10_u32),
            Decimal::from(150_u32),
            at,
            "".to_owned(),
        )?;
        assert_eq!(buy.kind, TransactionKind::Trade);
        assert_eq!(service.account(broker.id)?.balance, Decimal::from(8500_u32));
        let holdings = service.holdings()?;
        assert_eq!(holdings.len(), 1);
        assert_eq!(holdings[0].symbol, "AAPL");
        assert_eq!(holdings[0].shares, Decimal::from(10_u32));
        assert_eq!(holdings[0].cost_basis, Decimal::from(1500_u32));

        // 净资产：8500 现金 + 1500 持仓（默认摊薄成本）。
        let summary = service.balance_summary("CNY")?;
        assert_eq!(summary.total_assets, Decimal::from(10000_u32));

        // 设市价 200：持仓市值 2000，净资产 +500。
        service.set_holding_price(holdings[0].id, Decimal::from(200_u32))?;
        let summary = service.balance_summary("CNY")?;
        assert_eq!(summary.total_assets, Decimal::from(10500_u32));

        // 卖出 4 股 @ 200：现金 +800，持仓 6 股，成本 1500 - 4×150 = 900。
        let sell = service.sell_stock(
            broker.id,
            "AAPL".to_owned(),
            Decimal::from(4_u32),
            Decimal::from(200_u32),
            at,
            "".to_owned(),
        )?;
        assert_eq!(sell.kind, TransactionKind::Trade);
        assert_eq!(service.account(broker.id)?.balance, Decimal::from(9300_u32));
        let holdings = service.holdings()?;
        assert_eq!(holdings[0].shares, Decimal::from(6_u32));
        assert_eq!(holdings[0].cost_basis, Decimal::from(900_u32));

        // 清仓后无持仓。
        service.sell_stock(
            broker.id,
            "AAPL".to_owned(),
            Decimal::from(6_u32),
            Decimal::from(200_u32),
            at,
            "".to_owned(),
        )?;
        assert!(service.holdings()?.is_empty());
        Ok(())
    }

    #[test]
    fn balance_summary_converts_all_account_and_loan_currencies() -> Result<()> {
        let mut service = test_service()?;
        service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(100_u32))?;
        service.create_account("美元卡", AccountType::Credit, "USD", Decimal::from(20_u32))?;
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from(7_u32),
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;

        // 显示 CNY：资产 100，负债 20 USD×7 = 140，净资产 -40。
        let cny = service.balance_summary("CNY")?;
        assert_eq!(cny.total_assets, Decimal::from(100_u32));
        assert_eq!(cny.total_liabilities, Decimal::from(140_u32));
        assert_eq!(cny.net_worth, Decimal::from(-40_i32));
        // 显示 USD：资产 100÷7 ≈ 14.29，负债 20，净资产 -5.71。
        let usd = service.balance_summary("USD")?;
        assert_eq!(usd.total_assets.round_dp(2), Decimal::from_str("14.29")?);
        assert_eq!(usd.total_liabilities, Decimal::from(20_u32));
        assert_eq!(usd.net_worth.round_dp(2), Decimal::from_str("-5.71")?);
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
    fn expense_can_be_edited_and_balance_adjusts() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(500_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let transit = service.create_category("交通", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 10)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "午餐")?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(400_u32));

        // 改备注/分类/时间：余额不变。
        let later = at + ChronoDuration::days(3);
        service.update_transaction(
            expense.id,
            Some("晚餐".to_owned()),
            Some(later),
            Some(transit.id),
            None,
            None,
            None,
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(400_u32));
        let edited = service.transaction(expense.id)?;
        assert_eq!(edited.note, "晚餐");
        assert_eq!(edited.category_id, Some(transit.id));
        assert_eq!(edited.occurred_at, later);

        // 改金额 100 → 150：余额 400 → 350。
        service.update_transaction(
            expense.id,
            None,
            None,
            None,
            Some(Decimal::from(150_u32)),
            None,
            None,
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(350_u32));
        // 改回 100。
        service.update_transaction(
            expense.id,
            None,
            None,
            None,
            Some(Decimal::from(100_u32)),
            None,
            None,
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(400_u32));
        // 只改备注：余额不变。
        service.update_transaction(
            expense.id,
            Some("更正".to_owned()),
            None,
            None,
            None,
            None,
            None,
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(400_u32));
        Ok(())
    }

    #[test]
    fn income_edit_moves_balance_between_same_currency_accounts() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(100_u32))?;
        let savings =
            service.create_account("储蓄", AccountType::Savings, "CNY", Decimal::from(1000_u32))?;
        let salary = service.create_category("工资", CategoryKind::Income)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 10)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let income =
            service.record_income(cash.id, salary.id, Decimal::from(200_u32), at, "工资")?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(300_u32));

        // 从零钱移到储蓄：零钱 100、储蓄 1200。
        service.update_transaction(income.id, None, None, None, None, Some(savings.id), None)?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(100_u32));
        assert_eq!(
            service.account(savings.id)?.balance,
            Decimal::from(1200_u32)
        );
        assert_eq!(service.transaction(income.id)?.account_id, savings.id);
        Ok(())
    }

    #[test]
    fn editing_credit_expense_keeps_liability_direction() -> Result<()> {
        let mut service = test_service()?;
        let credit = service.create_account(
            "信用卡",
            AccountType::Credit,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 10)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        // 信用账户：支出增加欠款。
        let expense =
            service.record_expense(credit.id, food.id, Decimal::from(100_u32), at, "刷卡")?;
        assert_eq!(service.account(credit.id)?.balance, Decimal::from(1100_u32));
        // 金额改 250：欠款 1100 → 1250。
        service.update_transaction(
            expense.id,
            None,
            None,
            None,
            Some(Decimal::from(250_u32)),
            None,
            None,
        )?;
        assert_eq!(service.account(credit.id)?.balance, Decimal::from(1250_u32));
        Ok(())
    }

    #[test]
    fn edit_is_rejected_for_voided_loans_transfers_and_reimbursed() -> Result<()> {
        let mut service = test_service()?;
        let cash =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(1000_u32))?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 10)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();

        // 已撤销的流水不可编辑。
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "餐费")?;
        service.void_transaction(expense.id)?;
        assert!(service
            .update_transaction(
                expense.id,
                Some("x".to_owned()),
                None,
                None,
                None,
                None,
                None
            )
            .is_err());

        // 转账/借款流水不可编辑。
        let usd = service.create_account("美元", AccountType::Cash, "USD", Decimal::ZERO)?;
        let transfer = service.record_transfer(
            cash.id,
            usd.id,
            Decimal::from(10_u32),
            Decimal::from(10_u32),
            at,
            "换汇",
        )?;
        assert!(service
            .update_transaction(transfer.id, None, None, None, None, None, None)
            .is_err());
        let loan = service.create_loan(
            crate::domain::LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(100_u32),
            cash.id,
            "借款",
            None,
        )?;
        let loan_tx = service
            .transactions(100, 0)?
            .into_iter()
            .find(|item| item.loan_id == Some(loan.id))
            .ok_or_else(|| KokuError::InvalidInput("loan transaction not found".to_owned()))?;
        assert!(service
            .update_transaction(loan_tx.id, None, None, None, None, None, None)
            .is_err());

        // 已报销的支出只能改备注/分类/时间。
        let expense2 =
            service.record_expense(cash.id, food.id, Decimal::from(200_u32), at, "出差")?;
        service.mark_reimbursable(expense2.id)?;
        service.reimburse(
            expense2.id,
            cash.id,
            Decimal::from(200_u32),
            "CNY",
            None,
            "报销",
        )?;
        assert!(service
            .update_transaction(
                expense2.id,
                None,
                None,
                None,
                Some(Decimal::from(150_u32)),
                None,
                None
            )
            .is_err());
        service.update_transaction(
            expense2.id,
            Some("改备注".to_owned()),
            None,
            None,
            None,
            None,
            None,
        )?;

        // 报销收入流水也不可改金额。
        let income = service
            .transactions(100, 0)?
            .into_iter()
            .find(|item| item.kind == TransactionKind::Income)
            .ok_or_else(|| KokuError::InvalidInput("reimbursement income not found".to_owned()))?;
        assert!(service
            .update_transaction(
                income.id,
                None,
                None,
                None,
                Some(Decimal::from(300_u32)),
                None,
                None
            )
            .is_err());

        // 跨币种迁移账户被拒绝。
        let expense3 =
            service.record_expense(cash.id, food.id, Decimal::from(50_u32), at, "跨币种")?;
        assert!(service
            .update_transaction(expense3.id, None, None, None, None, Some(usd.id), None)
            .is_err());
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
    fn reimbursement_income_is_backdated_to_the_expense_month() -> Result<()> {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "出差餐费")?;
        service.mark_reimbursable(expense.id)?;

        // 报销收入的入账日期跟随支出日期，而不是报销当天。
        let income = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(100_u32),
            "CNY",
            None,
            "报销",
        )?;
        assert_eq!(income.occurred_at, expense.occurred_at);

        // 跨月场景：支出在 8 月，若在 9 月才报销，报表仍按 8 月归集，
        // 8 月的支出与报销收入同月相抵，9 月不出现追溯改写。
        let august = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(august.total_expense, Decimal::ZERO);
        assert_eq!(august.total_income, Decimal::from(100_u32));
        let september = service.monthly_summary(2026, 9, "CNY")?;
        assert_eq!(september.total_expense, Decimal::ZERO);
        assert_eq!(september.total_income, Decimal::ZERO);
        Ok(())
    }

    #[test]
    fn voiding_reimbursement_income_writes_back_the_expense() -> Result<()> {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "出差餐费")?;
        service.mark_reimbursable(expense.id)?;
        let income1 = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(40_u32),
            "CNY",
            None,
            "报销首笔",
        )?;
        let income2 = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(60_u32),
            "CNY",
            None,
            "报销尾款",
        )?;
        assert!(service.transaction(expense.id)?.reimbursed_at.is_some());

        // 撤销首笔报销收入：支出回写为「已报销 60、未全部报销」。
        service.void_transaction(income1.id)?;
        let expense_after = service.transaction(expense.id)?;
        assert_eq!(expense_after.reimbursed_amount, Decimal::from(60_u32));
        assert!(expense_after.reimbursed_at.is_none());
        // 余额只扣回被撤销的那一笔。
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(-40_i32));

        // 撤销第二笔：全部报销清零，可重新标记/报销。
        service.void_transaction(income2.id)?;
        let expense_done = service.transaction(expense.id)?;
        assert_eq!(expense_done.reimbursed_amount, Decimal::ZERO);
        assert!(expense_done.reimbursed_at.is_none());
        assert_eq!(service.account(cash.id)?.balance, Decimal::from(-100_i32));
        service.unmark_reimbursable(expense.id)?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.total_expense, Decimal::from(100_u32));
        assert_eq!(summary.total_income, Decimal::ZERO);
        Ok(())
    }

    #[test]
    fn voiding_reimbursed_expense_cascades_to_its_income_transactions() -> Result<()> {
        let mut service = test_service()?;
        let cash = service.create_account("零钱", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();
        let expense =
            service.record_expense(cash.id, food.id, Decimal::from(100_u32), at, "出差餐费")?;
        service.mark_reimbursable(expense.id)?;
        let income1 = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(40_u32),
            "CNY",
            None,
            "报销首笔",
        )?;
        let income2 = service.reimburse(
            expense.id,
            cash.id,
            Decimal::from(60_u32),
            "CNY",
            None,
            "报销尾款",
        )?;
        assert_eq!(service.account(cash.id)?.balance, Decimal::ZERO);

        // 先单独撤销一笔报销收入，再撤销整笔支出：级联跳过已撤销的收入。
        service.void_transaction(income1.id)?;
        service.void_transaction(expense.id)?;

        let expense_voided = service.transaction(expense.id)?;
        assert!(expense_voided.voided_at.is_some());
        assert_eq!(expense_voided.reimbursed_amount, Decimal::ZERO);
        assert!(expense_voided.reimbursed_at.is_none());
        assert!(expense_voided.reimbursable_at.is_none());
        assert!(service.transaction(income1.id)?.voided_at.is_some());
        assert!(service.transaction(income2.id)?.voided_at.is_some());
        // 支出、报销收入全部撤销后余额回到起点，统计清零。
        assert_eq!(service.account(cash.id)?.balance, Decimal::ZERO);
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.total_expense, Decimal::ZERO);
        assert_eq!(summary.total_income, Decimal::ZERO);
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
            None,
        )?;
        let second = service.create_loan(
            LoanType::Lend,
            "张三",
            "CNY",
            Decimal::from(500_u32),
            savings.id,
            "再借",
            None,
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
            None,
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
            None,
        )?;
        service.create_loan(
            LoanType::Lend,
            "李四",
            "CNY",
            Decimal::from(100_u32),
            savings.id,
            "",
            None,
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
            None,
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
            None,
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
            None,
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
