//! 认证与会话持久化：会话创建、校验与删除。

use super::*;
use crate::auth::{generate_session_token, session_token_hash};

impl BookkeepingService {
    pub fn create_auth_session(
        &mut self,
        user_id: i64,
        username: &str,
        ttl_seconds: i64,
    ) -> Result<String> {
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
            "INSERT INTO auth_sessions(token_hash, user_id, username, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session_token_hash(&token),
                user_id,
                username,
                timestamp(now),
                expires_at
            ],
        )?;
        transaction.commit()?;
        Ok(token)
    }

    /// 根据会话令牌解析当前登录用户；会话过期或用户被停用时返回 None。
    pub fn authenticated_user(&self, token: &str) -> Result<Option<User>> {
        let row = self
            .conn
            .query_row(
                "SELECT u.id, u.username, u.password_hash, u.role, u.enabled, u.created_at, u.totp_enabled
                 FROM auth_sessions s
                 JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND u.enabled = 1",
                params![session_token_hash(token), Utc::now().timestamp()],
                user_row,
            )
            .optional()
            .map_err(KokuError::from)?;
        row.map(user_from_row).transpose()
    }

    pub fn delete_auth_session(&mut self, token: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM auth_sessions WHERE token_hash = ?1",
            [session_token_hash(token)],
        )?;
        Ok(())
    }
}
