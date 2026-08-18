//! 数据库 schema 初始化：建表与索引（幂等，`IF NOT EXISTS`）。

use rusqlite::Connection;

use crate::error::Result;

/// 建表与索引。旧库兼容性迁移见 [`super::migrations::run`]。
pub(super) fn initialize(conn: &Connection) -> Result<()> {
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

        CREATE TABLE IF NOT EXISTS deposits (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            source_account_id INTEGER NOT NULL REFERENCES accounts(id),
            amount            TEXT NOT NULL,
            currency          TEXT NOT NULL,
            rate              TEXT NOT NULL,
            term_days         INTEGER NOT NULL,
            opened_at         TEXT NOT NULL,
            maturity_at       TEXT NOT NULL,
            settled_at        TEXT,
            note              TEXT NOT NULL DEFAULT ''
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

        CREATE TABLE IF NOT EXISTS reconciliations (
            id                        INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id                INTEGER NOT NULL REFERENCES accounts(id),
            statement_date            TEXT NOT NULL,
            statement_balance         TEXT NOT NULL,
            book_balance              TEXT NOT NULL,
            status                    TEXT NOT NULL CHECK (status IN ('open', 'completed', 'cancelled')),
            opened_at                 TEXT NOT NULL,
            completed_at              TEXT,
            adjustment_transaction_id INTEGER REFERENCES transactions(id),
            note                      TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS transactions (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'loan', 'adjustment', 'trade', 'deposit')),
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
                (kind IN ('loan', 'adjustment', 'trade', 'deposit') AND category_id IS NULL
                 AND to_account_id IS NULL AND target_amount IS NULL)
            )
        );

        CREATE INDEX IF NOT EXISTS idx_transactions_month
            ON transactions(occurred_at, voided_at);
        CREATE INDEX IF NOT EXISTS idx_transactions_account
            ON transactions(account_id, to_account_id);

        CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            username      TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL CHECK (role IN ('admin', 'member')),
            enabled       INTEGER NOT NULL DEFAULT 1,
            created_at    TEXT NOT NULL,
            totp_secret       TEXT,
            totp_enabled      INTEGER NOT NULL DEFAULT 0,
            totp_pending_secret TEXT
        );

        CREATE TABLE IF NOT EXISTS auth_sessions (
            token_hash TEXT PRIMARY KEY,
            user_id    INTEGER REFERENCES users(id),
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
    Ok(())
}
