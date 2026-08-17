//! 用户管理与多用户迁移：users 表 CRUD、会话归属，以及把旧的单用户账本
//! 数据搬迁到管理员独立账本文件的一次性迁移。

use std::path::Path;

use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::*;
use crate::domain::{User, UserRole};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 创建用户；用户名唯一（1-32 字符），密码为已生成的 bcrypt 哈希。
    pub fn create_user(
        &mut self,
        username: &str,
        password_hash: &str,
        role: UserRole,
    ) -> Result<User> {
        let username = username.trim();
        if username.is_empty() || username.chars().count() > 32 {
            return Err(KokuError::InvalidInput(
                "username must be 1-32 characters".to_owned(),
            ));
        }
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO users(username, password_hash, role, enabled, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![username, password_hash, role.as_str(), timestamp(now)],
        )?;
        let id = self.conn.last_insert_rowid();
        self.user(id)
    }

    pub fn user(&self, id: i64) -> Result<User> {
        let row = self
            .conn
            .query_row(
                "SELECT id, username, password_hash, role, enabled, created_at, totp_enabled
                 FROM users WHERE id = ?1",
                [id],
                user_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound { entity: "user", id })?;
        user_from_row(row)
    }

    pub fn user_by_username(&self, username: &str) -> Result<Option<User>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, username, password_hash, role, enabled, created_at, totp_enabled
                 FROM users WHERE username = ?1",
                [username.trim()],
                user_row,
            )
            .optional()?;
        row.map(user_from_row).transpose()
    }

    pub fn users(&self) -> Result<Vec<User>> {
        let mut statement = self.conn.prepare(
            "SELECT id, username, password_hash, role, enabled, created_at, totp_enabled
             FROM users ORDER BY id",
        )?;
        let rows = statement.query_map([], user_row)?;
        rows.map(|row| user_from_row(row?)).collect()
    }

    /// 设置密码（bcrypt 哈希）并作废该用户全部会话。
    pub fn set_user_password(&mut self, user_id: i64, password_hash: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE users SET password_hash = ?1 WHERE id = ?2",
            params![password_hash, user_id],
        )?;
        tx.execute("DELETE FROM auth_sessions WHERE user_id = ?1", [user_id])?;
        tx.commit()?;
        Ok(())
    }

    /// 启用/停用用户；停用时立即作废其全部会话。
    pub fn set_user_enabled(&mut self, user_id: i64, enabled: bool) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE users SET enabled = ?1 WHERE id = ?2",
            params![i64::from(enabled), user_id],
        )?;
        if !enabled {
            tx.execute("DELETE FROM auth_sessions WHERE user_id = ?1", [user_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 删除用户及其全部会话。
    pub fn delete_user(&mut self, user_id: i64) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM auth_sessions WHERE user_id = ?1", [user_id])?;
        tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
        tx.commit()?;
        Ok(())
    }

    /// 保存待启用的 TOTP 密钥（启用前不生效）。
    pub fn set_user_totp_pending(&mut self, user_id: i64, secret: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET totp_pending_secret = ?1 WHERE id = ?2",
            params![secret, user_id],
        )?;
        Ok(())
    }

    /// 读取待启用的 TOTP 密钥；用户不存在时报错，未生成过返回 `None`。
    pub fn user_totp_pending(&self, user_id: i64) -> Result<Option<String>> {
        self.user(user_id)?;
        Ok(self.conn.query_row(
            "SELECT totp_pending_secret FROM users WHERE id = ?1",
            [user_id],
            |row| row.get::<_, Option<String>>(0),
        )?)
    }

    /// 启用 TOTP：把待启用密钥转为正式密钥并置 enabled=1。
    pub fn enable_user_totp(&mut self, user_id: i64, secret: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE users SET totp_secret = ?1, totp_enabled = 1, totp_pending_secret = NULL WHERE id = ?2",
            params![secret, user_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 关闭 TOTP：清除密钥并置 enabled=0。
    pub fn disable_user_totp(&mut self, user_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET totp_secret = NULL, totp_enabled = 0, totp_pending_secret = NULL WHERE id = ?1",
            [user_id],
        )?;
        Ok(())
    }

    /// 读取正式启用的 TOTP 密钥（仅服务端内部使用，不对外暴露）。
    pub fn user_totp_secret(&self, user_id: i64) -> Result<Option<String>> {
        self.user(user_id)?;
        Ok(self.conn.query_row(
            "SELECT totp_secret FROM users WHERE id = ?1",
            [user_id],
            |row| row.get::<_, Option<String>>(0),
        )?)
    }
}

/// 多用户迁移：确保 users 表就绪、admin 用户存在、会话关联 user_id，
/// 并把旧的单用户账本数据搬迁到 admin 的独立账本文件。返回 admin 用户 id。
///
/// 幂等：users 已存在用户时不再引导；admin 账本文件已存在时不再搬迁。
pub fn ensure_multi_user(
    auth: &mut BookkeepingService,
    source_path: &Path,
    ledger_dir: &Path,
    admin_username: &str,
    bootstrap_password_hash: &str,
) -> Result<i64> {
    let admin_id = match auth.users()?.first().cloned() {
        Some(first) => first.id,
        None => {
            // 应用内改过的密码哈希优先，否则回退到环境/文件配置的初始哈希，
            // 保证迁移后管理员仍用当前密码登录。
            let hash = auth
                .get_setting("password_hash")?
                .unwrap_or_else(|| bootstrap_password_hash.to_owned());
            auth.create_user(admin_username, &hash, UserRole::Admin)?.id
        }
    };

    // 旧会话按 username 回填 user_id。
    auth.conn.execute(
        "UPDATE auth_sessions
         SET user_id = (SELECT id FROM users WHERE users.username = auth_sessions.username)
         WHERE user_id IS NULL",
        [],
    )?;

    // 单用户账本搬迁：admin 账本文件不存在、且共享库仍持有旧数据时复制过去。
    let ledger_path = ledger_dir.join(format!("ledger-{admin_id}.db"));
    if !ledger_path.exists() {
        let legacy_count: i64 =
            auth.conn
                .query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))?;
        if legacy_count > 0 {
            migrate_legacy_ledger(source_path, &ledger_path)?;
        }
    }
    Ok(admin_id)
}

/// 把旧共享库中的全部账本数据表复制到 admin 的独立账本文件。
/// 复制顺序满足外键依赖；复制后共享库中的旧数据表保留但不再被使用。
fn migrate_legacy_ledger(source_path: &Path, ledger_path: &Path) -> Result<()> {
    if let Some(parent) = ledger_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ledger = BookkeepingService::open(ledger_path)?;
    ledger
        .conn
        .execute("ATTACH DATABASE ?1 AS src", [source_path.to_string_lossy()])?;
    // 表顺序满足外键引用（accounts/categories 在前，transactions 在后）。
    for table in [
        "accounts",
        "categories",
        "tags",
        "loans",
        "deposits",
        "holdings",
        "budgets",
        "recurring_rules",
        "transactions",
        "transaction_tags",
        "receipts",
        "reimbursements",
    ] {
        ledger.conn.execute(
            &format!("INSERT INTO {table} SELECT * FROM src.{table}"),
            [],
        )?;
    }
    ledger.conn.execute("DETACH DATABASE src", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::BookkeepingService;

    #[test]
    fn totp_secret_lifecycle_pending_then_enable_then_disable() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let user = service.create_user("alice", "hash", UserRole::Admin)?;
        assert!(!service.user(user.id)?.totp_enabled);
        assert_eq!(service.user_totp_pending(user.id)?, None);

        service.set_user_totp_pending(user.id, "MZXW6YTBOI")?;
        assert_eq!(
            service.user_totp_pending(user.id)?.as_deref(),
            Some("MZXW6YTBOI")
        );
        // 未启用时正式密钥为空
        assert_eq!(service.user_totp_secret(user.id)?, None);
        assert!(!service.user(user.id)?.totp_enabled);

        service.enable_user_totp(user.id, "MZXW6YTBOI")?;
        let enabled = service.user(user.id)?;
        assert!(enabled.totp_enabled);
        assert_eq!(
            service.user_totp_secret(user.id)?.as_deref(),
            Some("MZXW6YTBOI")
        );
        // 启用后 pending 清空
        assert_eq!(service.user_totp_pending(user.id)?, None);

        service.disable_user_totp(user.id)?;
        assert!(!service.user(user.id)?.totp_enabled);
        assert_eq!(service.user_totp_secret(user.id)?, None);
        Ok(())
    }
}
