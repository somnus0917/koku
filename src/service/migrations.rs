//! 旧数据库兼容迁移：补列、整表重建与数据修复（幂等）。

use rusqlite::{params, Connection, OptionalExtension};

use super::parse_timestamp;
use crate::error::Result;

/// 迁移入口：仅在 [`super::schema::initialize`] 建表之后运行。
pub(super) fn run(conn: &Connection) -> Result<()> {
    if !table_has_column(conn, "transactions", "currency")? {
        conn.execute("ALTER TABLE transactions ADD COLUMN currency TEXT", [])?;
    }
    if !table_has_column(conn, "categories", "archived_at")? {
        conn.execute("ALTER TABLE categories ADD COLUMN archived_at TEXT", [])?;
    }
    if !table_has_column(conn, "transactions", "target_currency")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN target_currency TEXT",
            [],
        )?;
    }
    if !table_has_column(conn, "transactions", "settled_amount")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN settled_amount TEXT",
            [],
        )?;
    }
    // —— 账户模型扩展（定期/报销/借款）的幂等迁移 ——
    if !table_has_column(conn, "accounts", "interest_rate")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN interest_rate TEXT", [])?;
    }
    if !table_has_column(conn, "accounts", "maturity_at")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN maturity_at TEXT", [])?;
    }
    if !table_has_column(conn, "accounts", "credit_limit")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN credit_limit TEXT", [])?;
    }
    if !table_has_column(conn, "transactions", "loan_id")? {
        conn.execute("ALTER TABLE transactions ADD COLUMN loan_id INTEGER", [])?;
    }
    if !table_has_column(conn, "loans", "due_at")? {
        conn.execute("ALTER TABLE loans ADD COLUMN due_at TEXT", [])?;
    }
    if !table_has_column(conn, "transactions", "reimbursable_at")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN reimbursable_at TEXT",
            [],
        )?;
    }
    if !table_has_column(conn, "transactions", "reimbursed_at")? {
        conn.execute("ALTER TABLE transactions ADD COLUMN reimbursed_at TEXT", [])?;
    }
    if !table_has_column(conn, "transactions", "reimbursed_amount")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN reimbursed_amount TEXT NOT NULL DEFAULT '0'",
            [],
        )?;
    }
    // —— 多用户迁移：auth_sessions 关联 users ——
    if !table_has_column(conn, "auth_sessions", "user_id")? {
        conn.execute(
            "ALTER TABLE auth_sessions ADD COLUMN user_id INTEGER REFERENCES users(id)",
            [],
        )?;
    }
    // —— TOTP 二步验证列 ——
    if !table_has_column(conn, "users", "totp_secret")? {
        conn.execute("ALTER TABLE users ADD COLUMN totp_secret TEXT", [])?;
    }
    if !table_has_column(conn, "users", "totp_enabled")? {
        conn.execute(
            "ALTER TABLE users ADD COLUMN totp_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !table_has_column(conn, "users", "totp_pending_secret")? {
        conn.execute("ALTER TABLE users ADD COLUMN totp_pending_secret TEXT", [])?;
    }
    // SQLite 无法修改 CHECK 约束：旧表按 asset/liability 建模，需要整表重建。
    // 检测依据：表定义中缺少新类型标记（'cash'/'loan'），无论是否有旧 CHECK。
    if !table_sql_contains(conn, "accounts", "'cash'")? {
        rebuild_accounts_table(conn)?;
    }
    if !table_sql_contains(conn, "transactions", "'adjustment'")? {
        rebuild_transactions_table(conn)?;
    }
    if !table_sql_contains(conn, "transactions", "'trade'")? {
        rebuild_transactions_table(conn)?;
    }
    if !table_sql_contains(conn, "transactions", "'deposit'")? {
        rebuild_transactions_table(conn)?;
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
    migrate_deposit_accounts(conn)?;
    // —— Payee 自动学习：交易关联商户与原始描述（放在整表重建之后，避免重建丢失新列）——
    if !table_has_column(conn, "transactions", "payee_id")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN payee_id INTEGER REFERENCES payees(id)",
            [],
        )?;
    }
    if !table_has_column(conn, "transactions", "raw_description")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN raw_description TEXT",
            [],
        )?;
    }
    if !table_has_column(conn, "transactions", "import_external_id")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN import_external_id TEXT",
            [],
        )?;
    }
    // —— 信用卡账单模型：账单日 / 还款日（放在账户整表重建之后，避免重建丢失新列）——
    if !table_has_column(conn, "accounts", "statement_day")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN statement_day INTEGER", [])?;
    }
    if !table_has_column(conn, "accounts", "due_day")? {
        conn.execute("ALTER TABLE accounts ADD COLUMN due_day INTEGER", [])?;
    }
    Ok(())
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

/// 把旧的「定期即账户」模型迁移到独立 deposits 表：带利率标记的储蓄账户转为存款记录，
/// 本金从账户余额移入 deposits，账户归零并清掉利率标记（保留为普通储蓄账户）。
pub(super) fn migrate_deposit_accounts(conn: &Connection) -> Result<()> {
    let legacy = {
        let mut statement = conn.prepare(
            "SELECT id, currency, balance, interest_rate, maturity_at, created_at
             FROM accounts WHERE interest_rate IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut list = Vec::new();
        for row in rows {
            list.push(row?);
        }
        list
    };

    for (id, currency, balance, rate, maturity_at, created_at) in legacy {
        // 源账户 = 当初转入这笔定期的 transfer 流水的转出账户。
        let source: Option<i64> = conn
            .query_row(
                "SELECT account_id FROM transactions WHERE to_account_id = ?1 AND kind = 'transfer' ORDER BY id LIMIT 1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(source_account_id) = source else {
            tracing::warn!(target: "migration", deposit_account = id, "skipping legacy fixed deposit: no source transfer found");
            continue;
        };
        let opened = parse_timestamp(&created_at)?;
        let maturity = parse_timestamp(&maturity_at)?;
        let term_days = (maturity - opened).num_days().max(1) as u32;
        conn.execute(
            "INSERT INTO deposits(source_account_id, amount, currency, rate, term_days, opened_at, maturity_at, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '')",
            params![source_account_id, balance, currency, rate, term_days, created_at, maturity_at],
        )?;
        conn.execute(
            "UPDATE accounts SET balance = '0', interest_rate = NULL, maturity_at = NULL WHERE id = ?1",
            [id],
        )?;
    }
    Ok(())
}
