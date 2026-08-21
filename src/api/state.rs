//! API 共享基础设施：应用状态、鉴权用户信息、账本锁与持有句柄。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::auth::AuthConfig;
use crate::domain::UserRole;
use crate::error::{KokuError, Result};
use crate::quotes::QuoteClient;
use crate::r2::R2Client;
use crate::ratelimit::ApiRateLimiter;
use crate::rates::RateClient;
use crate::service::BookkeepingService;
use crate::throttle::LoginThrottle;

#[derive(Clone)]
pub struct AppState {
    /// 全局维护闸门：普通 API 请求持读锁；备份/恢复持写锁，避免恢复时旧连接继续写入。
    pub maintenance: Arc<tokio::sync::RwLock<()>>,
    /// 共享库（users / 会话 / 设置）。
    pub auth: Arc<Mutex<BookkeepingService>>,
    /// 每用户账本连接缓存（按 user_id；打开后复用，同一用户串行访问）。
    pub ledgers: Arc<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<BookkeepingService>>>>>,
    /// 独立账本文件目录。
    pub ledger_dir: PathBuf,
    /// 共享库文件路径（备份/恢复用）。
    pub db_path: PathBuf,
    pub auth_config: Arc<AuthConfig>,
    pub login_throttle: Arc<Mutex<LoginThrottle>>,
    /// 认证后业务接口的通用限流器。
    pub rate_limiter: Arc<Mutex<ApiRateLimiter>>,
    /// 等待第二步验证的登录：totp_token -> (user_id, 过期时间戳)。
    pub pending_totp: Arc<Mutex<HashMap<String, (i64, i64)>>>,
    pub rates: Arc<RateClient>,
    /// 持仓市价客户端（Stooq）。
    pub quotes: Arc<QuoteClient>,
    /// R2 异地备份客户端；未配置 KOKU_R2_* 时为 None。
    pub r2: Option<Arc<R2Client>>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiResponse<T> {
    data: T,
}

impl<T> ApiResponse<T> {
    pub(super) fn new(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AuthenticatedUser {
    pub(super) user_id: i64,
    pub(super) username: String,
    pub(super) role: UserRole,
    pub(super) totp_enabled: bool,
}

impl AuthenticatedUser {
    pub(super) fn require_admin(&self) -> Result<()> {
        if self.role != UserRole::Admin {
            return Err(KokuError::Forbidden);
        }
        Ok(())
    }
}

pub(super) fn lock_auth(state: &AppState) -> Result<MutexGuard<'_, BookkeepingService>> {
    state
        .auth
        .lock()
        .map_err(|_| KokuError::InvalidInput("auth service lock was poisoned".to_owned()))
}

/// 某用户账本服务的持锁句柄：持有「该用户的账本连接锁」（owned），
/// 既复用连接，也天然串行化同一用户的读写（SQLite 单写者）。
pub struct LedgerGuard {
    _connection: tokio::sync::OwnedMutexGuard<BookkeepingService>,
}

impl std::ops::Deref for LedgerGuard {
    type Target = BookkeepingService;
    fn deref(&self) -> &BookkeepingService {
        &self._connection
    }
}

impl std::ops::DerefMut for LedgerGuard {
    fn deref_mut(&mut self) -> &mut BookkeepingService {
        &mut self._connection
    }
}

/// 取得某用户的账本服务：命中缓存直接复用连接（同一用户串行访问）；
/// 首次访问时在 `spawn_blocking` 里打开/创建独立账本文件（schema 初始化
/// 与迁移不进异步 worker 线程），并补齐默认分类。
pub(super) async fn lock_ledger(state: &AppState, user_id: i64) -> Result<LedgerGuard> {
    let cached = {
        let map = state
            .ledgers
            .lock()
            .map_err(|_| KokuError::InvalidInput("ledger cache lock was poisoned".to_owned()))?;
        map.get(&user_id).cloned()
    };
    if let Some(ledger) = cached {
        let guard = ledger.lock_owned().await;
        return Ok(LedgerGuard { _connection: guard });
    }

    // 未缓存：建连（含 schema/迁移）放到阻塞线程，避免拖住异步 worker。
    let path = state.ledger_dir.join(format!("ledger-{user_id}.db"));
    let opened = tokio::task::spawn_blocking(move || -> Result<BookkeepingService> {
        let mut ledger = BookkeepingService::open(&path)?;
        ledger.ensure_default_categories()?;
        Ok(ledger)
    })
    .await
    .map_err(|error| KokuError::InvalidInput(format!("ledger open task failed: {error}")))??;

    // 把 map 锁的作用域收窄到克隆 Arc 为止，避免 std MutexGuard 跨 await 持有。
    let ledger = {
        let mut map = state
            .ledgers
            .lock()
            .map_err(|_| KokuError::InvalidInput("ledger cache lock was poisoned".to_owned()))?;
        map.entry(user_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(opened)))
            .clone()
    };
    let guard = ledger.lock_owned().await;
    Ok(LedgerGuard { _connection: guard })
}

#[cfg(test)]
mod send_check {
    use super::*;
    use crate::auth::AuthConfig;

    #[tokio::test]
    async fn ledger_guard_and_lock_future_are_send() {
        fn is_send<T: Send>() {}
        is_send::<BookkeepingService>();
        is_send::<LedgerGuard>();
        is_send::<AppState>();
        let state = AppState {
            maintenance: Arc::new(tokio::sync::RwLock::new(())),
            auth: Arc::new(Mutex::new(BookkeepingService::in_memory().unwrap())),
            ledgers: Arc::new(Mutex::new(HashMap::new())),
            ledger_dir: std::env::temp_dir(),
            db_path: std::env::temp_dir().join("koku-test.db"),
            auth_config: Arc::new(AuthConfig {
                username: String::from("t"),
                password_hash: String::from("h"),
                session_ttl_seconds: 3600,
                cookie_secure: false,
            }),
            login_throttle: Arc::new(Mutex::new(LoginThrottle::default())),
            rate_limiter: Arc::new(Mutex::new(ApiRateLimiter::default())),
            pending_totp: Arc::new(Mutex::new(HashMap::new())),
            rates: Arc::new(RateClient::new()),
            quotes: Arc::new(QuoteClient::new()),
            r2: None,
        };
        fn assert_send<F: std::future::Future + Send>(_: F) {}
        assert_send(lock_ledger(&state, 1));
    }
}
