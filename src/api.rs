//! REST API：请求/响应 DTO、鉴权中间件、处理器与路由。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{
    ConnectInfo, DefaultBodyLimit, Extension, Multipart, Path as AxumPath, Query, Request, State,
};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{generate_session_token, session_cookie, session_token, AuthConfig};
use crate::backup::{self, BackupMeta};
use crate::domain::{
    Account, AccountType, AuthSession, BalanceSummary, Budget, CashFlowSummary, Category,
    CategoryKind, Deposit, DepositSettlement, Holding, Loan, LoanType, MonthlySummary,
    MonthlyTrendPoint, RateQuote, Receipt, Reconciliation, RecurrenceFrequency, RecurringRule,
    RollingSummary, Tag, TagSummary, Transaction, TransactionKind, User, UserRole, YearlySummary,
};
use crate::error::{KokuError, Result};
use crate::importer::{self, ImportFormat};
use crate::mailer;
use crate::quotes::QuoteClient;
use crate::ratelimit::{rate_limit, ApiRateLimiter};
use crate::rates::{rate_is_fresh, RateClient};
use crate::service::{
    normalize_currency, reminder_digest_text, BookkeepingService, ImportResult, ReminderItem,
};
use crate::throttle::LoginThrottle;
use crate::totp;

#[derive(Clone)]
pub struct AppState {
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
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    data: T,
}

impl<T> ApiResponse<T> {
    fn new(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AuthenticatedUser {
    user_id: i64,
    username: String,
    role: UserRole,
    totp_enabled: bool,
}

impl AuthenticatedUser {
    fn require_admin(&self) -> Result<()> {
        if self.role != UserRole::Admin {
            return Err(KokuError::Forbidden);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    name: String,
    account_type: AccountType,
    currency: String,
    opening_balance: Decimal,
    /// 信用额度（仅信用账户）
    credit_limit: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
struct CreateCategoryRequest {
    name: String,
    kind: CategoryKind,
}

#[derive(Debug, Deserialize)]
struct CreateTransactionRequest {
    kind: TransactionKind,
    account_id: i64,
    category_id: i64,
    amount: Decimal,
    currency: Option<String>,
    settled_amount: Option<Decimal>,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    tag_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CreateTransferRequest {
    from_account_id: i64,
    to_account_id: i64,
    source_amount: Decimal,
    target_amount: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

/// TOTP 第二步验证：一次性令牌 + 6 位动态码。
#[derive(Debug, Deserialize)]
struct TotpVerifyRequest {
    totp_token: String,
    code: String,
}

/// TOTP 设置第一步：需要当前密码（防止他人远程开启二步验证锁死账号）。
#[derive(Debug, Deserialize)]
struct TotpSetupRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct TotpEnableRequest {
    code: String,
}

#[derive(Debug, Deserialize)]
struct TotpDisableRequest {
    code: String,
}

/// TOTP 待验证令牌有效期：5 分钟。
const TOTP_PENDING_TTL_SECONDS: i64 = 300;

#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

#[derive(Debug, Deserialize)]
struct MonthlyQuery {
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagSummaryQuery {
    /// 逗号分隔的标签名（AND 语义：交易须同时带有全部标签）。
    tags: String,
    /// 缺省时统计全部历史；year/month 必须同时给出或同时缺省。
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransactionQuery {
    /// 返回条数，默认 500，上限 1000（由 service 校验）。
    limit: Option<u32>,
    /// 跳过条数，默认 0。
    offset: Option<u32>,
    /// 与 `month` 成对出现时，只返回该自然月的流水。
    year: Option<i32>,
    month: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TrendQuery {
    /// 返回最近多少个月，默认 12，上限 120（由 service 校验）。
    months: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TrendQueryWithWindow {
    /// 返回最近多少个月，默认 12，上限 120（由 service 校验）。
    months: Option<u32>,
    /// 滚动平均窗口（月），默认 3，上限 120（由 service 校验）。
    window: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateAccountRequest {
    name: Option<String>,
    account_type: Option<AccountType>,
    currency: Option<String>,
    #[serde(default)]
    credit_limit: Option<Option<Decimal>>,
}

#[derive(Debug, Deserialize)]
struct UpdateTransactionRequest {
    note: Option<String>,
    occurred_at: Option<DateTime<Utc>>,
    category_id: Option<i64>,
    amount: Option<Decimal>,
    account_id: Option<i64>,
    settled_amount: Option<Decimal>,
    /// 提供时整体替换标签；不提供则保持不变。
    tag_names: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AdjustBalanceRequest {
    /// 带符号增量：正数增加余额、负数减少余额
    amount: Decimal,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct CreateDepositRequest {
    from_account_id: i64,
    amount: Decimal,
    currency: Option<String>,
    /// 利率（百分比，如 2.10 = 2.10%）
    rate: Decimal,
    term_days: u32,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct SettleDepositRequest {
    to_account_id: i64,
}

#[derive(Debug, Deserialize)]
struct ReimburseRequest {
    expense_id: i64,
    account_id: i64,
    amount: Decimal,
    currency: Option<String>,
    settled_amount: Option<Decimal>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct CreateLoanRequest {
    loan_type: LoanType,
    counterparty: String,
    currency: Option<String>,
    amount: Decimal,
    account_id: i64,
    #[serde(default)]
    note: String,
    due_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct RepayLoanRequest {
    account_id: i64,
    amount: Decimal,
    currency: Option<String>,
    settled_amount: Option<Decimal>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct BalanceQuery {
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RateQuery {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct BudgetQuery {
    year: i32,
    month: u32,
}

#[derive(Debug, Deserialize)]
struct SetBudgetRequest {
    limit_amount: Decimal,
}

#[derive(Debug, Deserialize)]
struct CreateRecurringRequest {
    kind: TransactionKind,
    account_id: i64,
    category_id: i64,
    amount: Decimal,
    #[serde(default)]
    note: String,
    frequency: RecurrenceFrequency,
    next_due_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    year: Option<i32>,
    month: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TradeRequest {
    account_id: i64,
    symbol: String,
    shares: Decimal,
    price: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct SetPriceRequest {
    price: Decimal,
}

#[derive(Debug, Deserialize)]
struct CreateReconciliationRequest {
    account_id: i64,
    /// 对账单日期（YYYY-MM-DD）。
    statement_date: String,
    /// 对账单目标余额（带符号，语义与账户余额一致）。
    statement_balance: Decimal,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct ReconciliationQuery {
    account_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RemindersQuery {
    /// 未来多少天内到期（默认 30，上限 365）。
    days: Option<i64>,
}

fn lock_auth(state: &AppState) -> Result<MutexGuard<'_, BookkeepingService>> {
    state
        .auth
        .lock()
        .map_err(|_| KokuError::InvalidInput("auth service lock was poisoned".to_owned()))
}

/// 打开某用户的账本服务（按值返回，SQLite WAL 支持多连接并发）。
/// 每个用户一个独立账本文件，打开时补齐默认分类。
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
async fn lock_ledger(state: &AppState, user_id: i64) -> Result<LedgerGuard> {
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

/// 把单个单元格转成 CSV 字段：含逗号/引号/换行时用引号包裹并转义引号。
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// 对用户可控的自由文本做公式注入防护：以 `=` `+` `-` `@` 开头的字段前置 `'`，
/// 避免在 Excel/Sheets 打开时被当作公式执行。仅用于文本字段，不用于数字列。
fn neutralize_formula(value: &str) -> String {
    if matches!(value.as_bytes().first(), Some(b'=' | b'+' | b'-' | b'@')) {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

async fn require_auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let Some(token) = session_token(request.headers()) else {
        return KokuError::Unauthorized.into_response();
    };
    let user = match lock_auth(&state).and_then(|service| {
        service
            .authenticated_user(&token)?
            .ok_or(KokuError::Unauthorized)
    }) {
        Ok(user) => user,
        Err(error) => return error.into_response(),
    };
    request.extensions_mut().insert(AuthenticatedUser {
        user_id: user.id,
        username: user.username,
        role: user.role,
        totp_enabled: user.totp_enabled,
    });
    next.run(request).await
}

async fn api_login(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response> {
    let key = LoginThrottle::client_key(&headers, Some(remote.ip()));
    // 限流检查：同一来源在窗口内失败次数已达上限时直接 429（不执行 bcrypt）。
    let locked = state
        .login_throttle
        .lock()
        .map_err(|_| KokuError::InvalidInput("login throttle lock was poisoned".to_owned()))?
        .record(&key, false)
        .is_err();
    if locked {
        tracing::warn!(target: "auth", "login blocked by rate limit from {key}");
        return Err(KokuError::RateLimited);
    }

    let password = request.password;
    let user = lock_auth(&state)?
        .user_by_username(&request.username)?
        .ok_or(KokuError::InvalidCredentials)?;
    if !user.enabled {
        return Err(KokuError::InvalidCredentials);
    }
    let password_matches =
        tokio::task::spawn_blocking(move || bcrypt::verify(password, &user.password_hash))
            .await
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("password verification task failed: {error}"))
            })?
            .map_err(|error| KokuError::AuthConfiguration(error.to_string()))?;
    if !password_matches {
        tracing::warn!(target: "auth", "failed login attempt from {key}");
        return Err(KokuError::InvalidCredentials);
    }
    // 密码通过：清除该来源的失败计数。
    if let Ok(mut throttle) = state.login_throttle.lock() {
        let _ = throttle.record(&key, true);
    }
    tracing::info!(target: "auth", "password verified for {} from {key}", user.username);

    // TOTP 已启用：不直接发会话，返回一次性令牌进入第二步验证。
    if user.totp_enabled {
        let pending_token = generate_session_token()?;
        let expires = Utc::now().timestamp() + TOTP_PENDING_TTL_SECONDS;
        {
            let mut pending = state.pending_totp.lock().map_err(|_| {
                KokuError::InvalidInput("pending totp lock was poisoned".to_owned())
            })?;
            let now = Utc::now().timestamp();
            pending.retain(|_, (_, expiry)| *expiry > now);
            pending.insert(pending_token.clone(), (user.id, expires));
        }
        let mut response = Json(ApiResponse::new(serde_json::json!({
            "totp_required": true,
            "totp_token": pending_token,
            "username": user.username,
        })))
        .into_response();
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return Ok(response);
    }

    let token = lock_auth(&state)?.create_auth_session(
        user.id,
        &user.username,
        state.auth_config.session_ttl_seconds,
    )?;
    let mut response = Json(ApiResponse::new(AuthSession {
        id: user.id,
        username: user.username.clone(),
        role: user.role,
        totp_enabled: false,
    }))
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(
            &token,
            state.auth_config.session_ttl_seconds,
            state.auth_config.cookie_secure,
        )?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// TOTP 第二步：校验动态码并创建会话（一次性令牌 5 分钟内有效）。
async fn api_totp_verify(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<TotpVerifyRequest>,
) -> Result<Response> {
    let key = LoginThrottle::client_key(&headers, Some(remote.ip()));
    let locked = state
        .login_throttle
        .lock()
        .map_err(|_| KokuError::InvalidInput("login throttle lock was poisoned".to_owned()))?
        .record(&key, false)
        .is_err();
    if locked {
        tracing::warn!(target: "auth", "totp login blocked by rate limit from {key}");
        return Err(KokuError::RateLimited);
    }

    let (user_id, _) = {
        let mut pending = state
            .pending_totp
            .lock()
            .map_err(|_| KokuError::InvalidInput("pending totp lock was poisoned".to_owned()))?;
        let now = Utc::now().timestamp();
        pending.retain(|_, (_, expiry)| *expiry > now);
        pending
            .remove(&request.totp_token)
            .ok_or(KokuError::InvalidCredentials)?
    };
    let user = lock_auth(&state)?.user(user_id)?;
    let secret = lock_auth(&state)?
        .user_totp_secret(user.id)?
        .ok_or(KokuError::InvalidCredentials)?;
    let code = request.code;
    let valid = tokio::task::spawn_blocking(move || totp::verify_code(&secret, &code))
        .await
        .map_err(|error| KokuError::AuthConfiguration(format!("totp task failed: {error}")))??;
    if !valid {
        tracing::warn!(target: "auth", "failed totp attempt from {key}");
        return Err(KokuError::InvalidCredentials);
    }
    if let Ok(mut throttle) = state.login_throttle.lock() {
        let _ = throttle.record(&key, true);
    }
    tracing::info!(target: "auth", "totp verified for {} from {key}", user.username);

    let token = lock_auth(&state)?.create_auth_session(
        user.id,
        &user.username,
        state.auth_config.session_ttl_seconds,
    )?;
    let mut response = Json(ApiResponse::new(AuthSession {
        id: user.id,
        username: user.username.clone(),
        role: user.role,
        totp_enabled: true,
    }))
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(
            &token,
            state.auth_config.session_ttl_seconds,
            state.auth_config.cookie_secure,
        )?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// TOTP 设置第一步：校验当前密码后生成新密钥（暂存为 pending，未启用）。
async fn api_totp_setup(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<TotpSetupRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let current_hash = lock_auth(&state)?.user(user.user_id)?.password_hash;
    let password = request.password;
    let password_matches =
        tokio::task::spawn_blocking(move || bcrypt::verify(password, &current_hash))
            .await
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("password verification task failed: {error}"))
            })?
            .map_err(|error| KokuError::AuthConfiguration(error.to_string()))?;
    if !password_matches {
        return Err(KokuError::InvalidCredentials);
    }
    let secret = totp::generate_secret_base32()?;
    lock_auth(&state)?.set_user_totp_pending(user.user_id, &secret)?;
    let otpauth_uri = totp::otpauth_uri(&secret, "Koku", &user.username)?;
    Ok(Json(ApiResponse::new(serde_json::json!({
        "secret": secret,
        "otpauth_uri": otpauth_uri,
    }))))
}

/// TOTP 设置第二步：用动态码确认密钥可用后启用。
async fn api_totp_enable(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<TotpEnableRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let pending = lock_auth(&state)?
        .user_totp_pending(user.user_id)?
        .ok_or_else(|| {
            KokuError::InvalidInput("没有待启用的 TOTP 密钥，请先输入当前密码重新生成".to_owned())
        })?;
    let code = request.code;
    let pending_for_check = pending.clone();
    let valid = tokio::task::spawn_blocking(move || totp::verify_code(&pending_for_check, &code))
        .await
        .map_err(|error| KokuError::AuthConfiguration(format!("totp task failed: {error}")))??;
    if !valid {
        return Err(KokuError::InvalidInput("动态码不正确，请重试".to_owned()));
    }
    lock_auth(&state)?.enable_user_totp(user.user_id, &pending)?;
    tracing::info!(target: "auth", "totp enabled for {}", user.username);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "enabled": true }),
    )))
}

/// 关闭 TOTP：需要提供当前有效的动态码。
async fn api_totp_disable(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<TotpDisableRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let secret = lock_auth(&state)?
        .user_totp_secret(user.user_id)?
        .ok_or_else(|| KokuError::InvalidInput("当前未启用 TOTP".to_owned()))?;
    let code = request.code;
    let valid = tokio::task::spawn_blocking(move || totp::verify_code(&secret, &code))
        .await
        .map_err(|error| KokuError::AuthConfiguration(format!("totp task failed: {error}")))??;
    if !valid {
        return Err(KokuError::InvalidInput("动态码不正确，请重试".to_owned()));
    }
    lock_auth(&state)?.disable_user_totp(user.user_id)?;
    tracing::info!(target: "auth", "totp disabled for {}", user.username);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "enabled": false }),
    )))
}

async fn api_auth_session(Extension(user): Extension<AuthenticatedUser>) -> Response {
    let mut response = Json(ApiResponse::new(AuthSession {
        id: user.user_id,
        username: user.username,
        role: user.role,
        totp_enabled: user.totp_enabled,
    }))
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn api_logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let Some(token) = session_token(&headers) {
        lock_auth(&state)?.delete_auth_session(&token)?;
    }
    let mut response =
        Json(ApiResponse::new(serde_json::json!({ "logged_out": true }))).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie("", 0, state.auth_config.cookie_secure)?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn api_change_password(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let old_password = request.old_password;
    let new_password = request.new_password;
    if new_password.chars().count() < 8 {
        return Err(KokuError::InvalidInput(
            "new password must be at least 8 characters".to_owned(),
        ));
    }
    let current_hash = lock_auth(&state)?.user(user.user_id)?.password_hash;
    let old_matches =
        tokio::task::spawn_blocking(move || bcrypt::verify(old_password, &current_hash))
            .await
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("password verification task failed: {error}"))
            })?
            .map_err(|error| KokuError::AuthConfiguration(error.to_string()))?;
    if !old_matches {
        return Err(KokuError::InvalidCredentials);
    }
    let new_hash =
        tokio::task::spawn_blocking(move || bcrypt::hash(new_password, bcrypt::DEFAULT_COST))
            .await
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("password hashing task failed: {error}"))
            })?
            .map_err(|error| KokuError::AuthConfiguration(error.to_string()))?;

    lock_auth(&state)?.set_user_password(user.user_id, &new_hash)?;
    tracing::info!(target: "auth", "password changed for {}; sessions invalidated", user.username);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "changed": true }),
    )))
}

#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct ResetPasswordRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct SetUserEnabledRequest {
    enabled: bool,
}

/// 校验并生成密码哈希（bcrypt，代价默认）。
async fn hash_password(password: String) -> Result<String> {
    if password.chars().count() < 8 {
        return Err(KokuError::InvalidInput(
            "password must be at least 8 characters".to_owned(),
        ));
    }
    tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
        .await
        .map_err(|error| {
            KokuError::AuthConfiguration(format!("password hashing task failed: {error}"))
        })?
        .map_err(|error| KokuError::AuthConfiguration(error.to_string()))
}

async fn api_users(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<ApiResponse<Vec<User>>>> {
    user.require_admin()?;
    let users = lock_auth(&state)?.users()?;
    Ok(Json(ApiResponse::new(users)))
}

async fn api_create_user(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<ApiResponse<User>>)> {
    user.require_admin()?;
    if request.username.trim().is_empty() {
        return Err(KokuError::InvalidInput(
            "username cannot be empty".to_owned(),
        ));
    }
    if lock_auth(&state)?
        .user_by_username(&request.username)?
        .is_some()
    {
        return Err(KokuError::InvalidInput(
            "username already exists".to_owned(),
        ));
    }
    let hash = hash_password(request.password).await?;
    let created = lock_auth(&state)?.create_user(&request.username, &hash, UserRole::Member)?;
    tracing::info!(target: "auth", "admin {} created user {}", user.username, created.username);
    Ok((StatusCode::CREATED, Json(ApiResponse::new(created))))
}

async fn api_reset_user_password(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    AxumPath(user_id): AxumPath<i64>,
    Json(request): Json<ResetPasswordRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    let hash = hash_password(request.password).await?;
    lock_auth(&state)?.set_user_password(user_id, &hash)?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "changed": true }),
    )))
}

async fn api_set_user_enabled(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    AxumPath(user_id): AxumPath<i64>,
    Json(request): Json<SetUserEnabledRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    if user_id == user.user_id && !request.enabled {
        return Err(KokuError::InvalidInput(
            "cannot disable your own account".to_owned(),
        ));
    }
    let target = lock_auth(&state)?.user(user_id)?;
    if target.role == UserRole::Admin && !request.enabled {
        // 停用管理员时保留至少一个启用中的管理员。
        let enabled_admins = lock_auth(&state)?
            .users()?
            .into_iter()
            .filter(|item| item.role == UserRole::Admin && item.enabled)
            .count();
        if enabled_admins <= 1 {
            return Err(KokuError::InvalidInput(
                "cannot disable the last enabled admin".to_owned(),
            ));
        }
    }
    lock_auth(&state)?.set_user_enabled(user_id, request.enabled)?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "enabled": request.enabled }),
    )))
}

async fn api_delete_user(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    AxumPath(user_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    if user_id == user.user_id {
        return Err(KokuError::InvalidInput(
            "cannot delete your own account".to_owned(),
        ));
    }
    let target = lock_auth(&state)?.user(user_id)?;
    if target.role == UserRole::Admin {
        let admins = lock_auth(&state)?.users()?.len();
        if admins <= 1 {
            return Err(KokuError::InvalidInput(
                "cannot delete the last admin".to_owned(),
            ));
        }
    }
    lock_auth(&state)?.delete_user(user_id)?;
    // 连带删除该用户的独立账本文件（含 WAL/SHM）。
    for suffix in ["", "-wal", "-shm"] {
        let path = state
            .ledger_dir
            .join(format!("ledger-{user_id}.db{suffix}"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                KokuError::InvalidInput(format!("failed to remove ledger: {error}"))
            })?;
        }
    }
    tracing::info!(target: "auth", "admin {} deleted user {}", user.username, target.username);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "deleted": true }),
    )))
}

async fn api_health() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::new(serde_json::json!({
        "status": "ok",
        "service": "koku-api"
    })))
}

/// 列出全部备份（管理员）。
async fn api_list_backups(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<BackupMeta>>>> {
    user.require_admin()?;
    let backups = backup::list_backups(&state.db_path)?;
    Ok(Json(ApiResponse::new(backups)))
}

/// 手动创建一份备份（管理员）。
async fn api_create_backup(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<ApiResponse<BackupMeta>>)> {
    user.require_admin()?;
    // 默认保留最近 14 份；与定时任务共用同一清理策略。
    let keep = std::env::var("KOKU_BACKUP_KEEP")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(14);
    let meta = backup::create_backup(&state.db_path, &state.ledger_dir, keep)?;
    tracing::info!(target: "auth", "admin {} created backup {}", user.username, meta.id);
    Ok((StatusCode::CREATED, Json(ApiResponse::new(meta))))
}

/// 下载备份 zip（管理员）。
async fn api_download_backup(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Response> {
    user.require_admin()?;
    let dir = backup::backup_dir(&state.db_path);
    let path = dir.join(format!("koku-{backup_id}.zip"));
    let bytes = std::fs::read(&path)
        .map_err(|error| KokuError::InvalidInput(format!("backup not found: {error}")))?;
    let filename = format!("koku-{backup_id}.zip");
    let mut response = Response::new(axum::body::Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|error| KokuError::InvalidInput(format!("invalid filename: {error}")))?;
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, disposition);
    Ok(response)
}

/// 恢复备份（管理员）：覆盖共享库与全部账本文件，随后重开共享库连接并
/// 清空账本连接缓存。恢复会使当前所有会话失效（共享库被替换）。
async fn api_restore_backup(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    backup::restore_backup(&state.db_path, &state.ledger_dir, &backup_id)?;
    // 关闭全部账本连接缓存：下一次访问会基于恢复后的文件重新打开。
    state
        .ledgers
        .lock()
        .map_err(|_| KokuError::InvalidInput("ledger cache lock was poisoned".to_owned()))?
        .clear();
    // 重开共享库：替换运行中的连接（旧连接随之关闭），所有会话从新库读取后失效。
    {
        let mut guard = lock_auth(&state)?;
        *guard = BookkeepingService::open(&state.db_path)?;
    }
    tracing::info!(target: "auth", "admin {} restored backup {}", user.username, backup_id);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "restored": true }),
    )))
}

async fn api_accounts(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Account>>>> {
    let accounts = lock_ledger(&state, user.user_id).await?.accounts()?;
    Ok(Json(ApiResponse::new(accounts)))
}

async fn api_create_account(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Account>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.create_account(
        request.name,
        request.account_type,
        request.currency,
        request.opening_balance,
    )?;
    let account = match request.credit_limit {
        Some(limit) => service.set_credit_limit(account.id, Some(limit))?,
        None => account,
    };
    Ok((StatusCode::CREATED, Json(ApiResponse::new(account))))
}

async fn api_update_account(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<ApiResponse<Account>>> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.update_account(
        account_id,
        request.name,
        request.account_type,
        request.currency,
    )?;
    let account = match request.credit_limit {
        Some(limit) => service.set_credit_limit(account.id, limit)?,
        None => account,
    };
    Ok(Json(ApiResponse::new(account)))
}

async fn api_adjust_balance(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<AdjustBalanceRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.adjust_balance(
        account_id,
        request.amount,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_categories(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Category>>>> {
    let categories = lock_ledger(&state, user.user_id).await?.categories()?;
    Ok(Json(ApiResponse::new(categories)))
}

async fn api_create_category(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Category>>)> {
    let category = lock_ledger(&state, user.user_id)
        .await?
        .create_category(request.name, request.kind)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(category))))
}

async fn api_delete_category(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Category>>> {
    let category = lock_ledger(&state, user.user_id)
        .await?
        .delete_category(category_id)?;
    Ok(Json(ApiResponse::new(category)))
}

async fn api_tags(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Tag>>>> {
    let tags = lock_ledger(&state, user.user_id).await?.all_tags()?;
    Ok(Json(ApiResponse::new(tags)))
}

async fn api_budgets(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Vec<Budget>>>> {
    let budgets = lock_ledger(&state, user.user_id)
        .await?
        .budgets(query.year, query.month)?;
    Ok(Json(ApiResponse::new(budgets)))
}

async fn api_set_budget(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
    Json(request): Json<SetBudgetRequest>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget = lock_ledger(&state, user.user_id).await?.set_budget(
        category_id,
        query.year,
        query.month,
        request.limit_amount,
    )?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_clear_budget(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget = lock_ledger(&state, user.user_id).await?.clear_budget(
        category_id,
        query.year,
        query.month,
    )?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_rollover_budgets(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let copied = lock_ledger(&state, user.user_id)
        .await?
        .rollover_budgets_once(Utc::now())?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "copied": copied }),
    )))
}

async fn api_recurring_rules(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<RecurringRule>>>> {
    let rules = lock_ledger(&state, user.user_id).await?.recurring_rules()?;
    Ok(Json(ApiResponse::new(rules)))
}

async fn api_create_recurring(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateRecurringRequest>,
) -> Result<(StatusCode, Json<ApiResponse<RecurringRule>>)> {
    let rule = lock_ledger(&state, user.user_id)
        .await?
        .create_recurring_rule(
            request.kind,
            request.account_id,
            request.category_id,
            request.amount,
            request.note,
            request.frequency,
            request.next_due_at,
        )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(rule))))
}

async fn api_delete_recurring(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(rule_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<RecurringRule>>> {
    let rule = lock_ledger(&state, user.user_id)
        .await?
        .delete_recurring_rule(rule_id)?;
    Ok(Json(ApiResponse::new(rule)))
}

async fn api_run_recurring(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let generated = lock_ledger(&state, user.user_id).await?.run_recurring()?;
    Ok(Json(ApiResponse::new(generated)))
}

async fn api_holdings(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Holding>>>> {
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    Ok(Json(ApiResponse::new(holdings)))
}

async fn api_buy_stock(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<TradeRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.buy_stock(
        request.account_id,
        request.symbol,
        request.shares,
        request.price,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_sell_stock(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<TradeRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.sell_stock(
        request.account_id,
        request.symbol,
        request.shares,
        request.price,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_set_holding_price(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(holding_id): AxumPath<i64>,
    Json(request): Json<SetPriceRequest>,
) -> Result<Json<ApiResponse<Holding>>> {
    let holding = lock_ledger(&state, user.user_id)
        .await?
        .set_holding_price(holding_id, request.price)?;
    Ok(Json(ApiResponse::new(holding)))
}

/// 市价新鲜度阈值（小时）；`KOKU_QUOTE_TTL_HOURS` 可覆盖，默认 24。
fn quote_ttl_hours() -> i64 {
    std::env::var("KOKU_QUOTE_TTL_HOURS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(24)
}

/// 刷新全部过期（或缺失）市价：并发拉取 Stooq 后批量写回，返回逐标的明细。
async fn api_refresh_holdings(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    let ttl_hours = quote_ttl_hours();
    let now = Utc::now();
    let stale: Vec<(i64, String)> = holdings
        .iter()
        .filter(|holding| {
            holding
                .updated_at
                .map(|updated| now.signed_duration_since(updated).num_hours() >= ttl_hours)
                .unwrap_or(true)
        })
        .map(|holding| (holding.id, holding.symbol.clone()))
        .collect();

    // 并发拉取行情（持仓数量通常很少，JoinSet 足够）。
    let mut join_set = tokio::task::JoinSet::new();
    for (holding_id, symbol) in stale {
        let client = state.quotes.clone();
        join_set.spawn(async move {
            let result = client.fetch(&symbol).await;
            (holding_id, symbol, result)
        });
    }
    let mut refreshed = 0_usize;
    let mut failed: Vec<serde_json::Value> = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        let (holding_id, symbol, result) = joined.map_err(|error| {
            KokuError::InvalidInput(format!("quote fetch task failed: {error}"))
        })?;
        match result {
            Ok(quote) => {
                lock_ledger(&state, user.user_id)
                    .await?
                    .set_holding_price(holding_id, quote.price)?;
                refreshed += 1;
            }
            Err(error) => failed.push(serde_json::json!({
                "symbol": symbol,
                "error": error.to_string(),
            })),
        }
    }
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    Ok(Json(ApiResponse::new(serde_json::json!({
        "refreshed": refreshed,
        "failed": failed,
        "holdings": holdings,
    }))))
}

/// 刷新单只持仓市价（忽略 TTL，强制拉取）。
async fn api_refresh_holding(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(holding_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Holding>>> {
    let symbol = {
        let service = lock_ledger(&state, user.user_id).await?;
        service.holding(holding_id)?.symbol
    };
    let quote = state.quotes.fetch(&symbol).await?;
    let holding = lock_ledger(&state, user.user_id)
        .await?
        .set_holding_price(holding_id, quote.price)?;
    Ok(Json(ApiResponse::new(holding)))
}

async fn api_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let limit = query.limit.unwrap_or(500);
    let offset = query.offset.unwrap_or(0);
    let service = lock_ledger(&state, user.user_id).await?;
    let transactions = match (query.year, query.month) {
        (Some(year), Some(month)) => service.transactions_in_month(year, month, limit, offset)?,
        (None, None) => service.transactions(limit, offset)?,
        _ => {
            return Err(KokuError::InvalidInput(
                "year and month must be provided together".to_owned(),
            ))
        }
    };
    Ok(Json(ApiResponse::new(transactions)))
}

async fn api_export_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> Result<Response> {
    let service = lock_ledger(&state, user.user_id).await?;
    // 分页拉全量（复用既有 1000 上限，逐页累积）。
    let mut all = Vec::new();
    let mut offset = 0_u32;
    loop {
        let page = match (query.year, query.month) {
            (Some(year), Some(month)) => {
                service.transactions_in_month(year, month, 1000, offset)?
            }
            (None, None) => service.transactions(1000, offset)?,
            _ => {
                return Err(KokuError::InvalidInput(
                    "year and month must be provided together".to_owned(),
                ))
            }
        };
        let count = page.len();
        all.extend(page);
        offset += count as u32;
        if count < 1000 {
            break;
        }
    }

    let account_names: HashMap<i64, String> = service
        .accounts()?
        .into_iter()
        .map(|account| (account.id, account.name))
        .collect();
    let category_names: HashMap<i64, String> = service
        .categories()?
        .into_iter()
        .map(|category| (category.id, category.name))
        .collect();

    let mut csv = String::from(
        "id,kind,account,target_account,category,amount,currency,settled_amount,occurred_at,note,voided_at\n",
    );
    for tx in &all {
        let account = neutralize_formula(
            &account_names
                .get(&tx.account_id)
                .cloned()
                .unwrap_or_default(),
        );
        let target_account = neutralize_formula(
            &tx.to_account_id
                .and_then(|id| account_names.get(&id).cloned())
                .unwrap_or_default(),
        );
        let category = neutralize_formula(
            &tx.category_id
                .and_then(|id| category_names.get(&id).cloned())
                .unwrap_or_default(),
        );
        let note = neutralize_formula(&tx.note);
        let fields = [
            tx.id.to_string(),
            tx.kind.as_str().to_owned(),
            account,
            target_account,
            category,
            tx.amount.normalize().to_string(),
            tx.currency.clone(),
            tx.settled_amount.normalize().to_string(),
            tx.occurred_at.to_rfc3339(),
            note,
            tx.voided_at
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
        ];
        csv.push_str(
            &fields
                .iter()
                .map(|field| csv_field(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }

    let filename = match (query.year, query.month) {
        (Some(year), Some(month)) => format!("koku-transactions-{year}-{month:02}.csv"),
        _ => "koku-transactions.csv".to_owned(),
    };
    let mut response = Response::new(axum::body::Body::from(csv));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(|error| KokuError::InvalidInput(format!("invalid filename: {error}")))?;
    response
        .headers_mut()
        .insert(header::CONTENT_DISPOSITION, disposition);
    Ok(response)
}

async fn api_create_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.account(request.account_id)?;
    let currency = request.currency.unwrap_or_else(|| account.currency.clone());
    let settled_amount = match request.settled_amount {
        Some(amount) => amount,
        None if currency.eq_ignore_ascii_case(&account.currency) => request.amount,
        None => {
            return Err(KokuError::InvalidInput(format!(
                "settled_amount in {} is required for a {currency} transaction",
                account.currency
            )))
        }
    };
    let transaction = match request.kind {
        TransactionKind::Expense => service.record_expense_in_currency(
            request.account_id,
            request.category_id,
            request.amount,
            currency,
            settled_amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Income => service.record_income_in_currency(
            request.account_id,
            request.category_id,
            request.amount,
            currency,
            settled_amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Transfer => {
            return Err(KokuError::InvalidInput(
                "use /api/transfers for transfer transactions".to_owned(),
            ))
        }
        TransactionKind::Loan => {
            return Err(KokuError::InvalidInput(
                "use /api/loans for loan transactions".to_owned(),
            ))
        }
        TransactionKind::Adjustment => {
            return Err(KokuError::InvalidInput(
                "use /api/accounts/{id}/adjust-balance to adjust a balance".to_owned(),
            ))
        }
        TransactionKind::Trade => {
            return Err(KokuError::InvalidInput(
                "use /api/holdings/buy or /api/holdings/sell for trades".to_owned(),
            ))
        }
        TransactionKind::Deposit => {
            return Err(KokuError::InvalidInput(
                "use /api/deposits for fixed deposits".to_owned(),
            ))
        }
    };
    if !request.tag_names.is_empty() {
        service.set_transaction_tags(transaction.id, request.tag_names)?;
    }
    let transaction = service.transaction(transaction.id)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_create_transfer(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateTransferRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.record_transfer(
        request.from_account_id,
        request.to_account_id,
        request.source_amount,
        request.target_amount,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_void_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .void_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_restore_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .restore_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_delete_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&state, user.user_id)
        .await?
        .delete_transaction(transaction_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_update_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    Json(request): Json<UpdateTransactionRequest>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    service.update_transaction(
        transaction_id,
        request.note,
        request.occurred_at,
        request.category_id,
        request.amount,
        request.account_id,
        request.settled_amount,
    )?;
    if let Some(tag_names) = request.tag_names {
        service.set_transaction_tags(transaction_id, tag_names)?;
    }
    let transaction = service.transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

/// 批量导入流水（CSV/QIF/OFX）：multipart 字段
/// `file`（必填）、`format`（csv|qif|ofx|auto，缺省 auto）、`account_id`（必填）、
/// `category_id`（可选默认分类）、`currency`（可选默认币种）。
async fn api_import_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ApiResponse<ImportResult>>)> {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut format: Option<String> = None;
    let mut account_id: Option<i64> = None;
    let mut category_id: Option<i64> = None;
    let mut currency: Option<String> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| KokuError::InvalidInput(format!("invalid multipart upload: {error}")))?
    {
        match field.name() {
            Some("file") => {
                file_name = field.file_name().map(str::to_owned);
                let bytes = field.bytes().await.map_err(|error| {
                    KokuError::InvalidInput(format!("could not read upload: {error}"))
                })?;
                file_bytes = Some(bytes.to_vec());
            }
            Some("format") => format = Some(field.text().await.unwrap_or_default()),
            Some("account_id") => {
                account_id = field.text().await.ok().and_then(|value| value.parse().ok())
            }
            Some("category_id") => {
                category_id = field.text().await.ok().and_then(|value| value.parse().ok())
            }
            Some("currency") => currency = Some(field.text().await.unwrap_or_default()),
            _ => {}
        }
    }
    let file_bytes = file_bytes.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"file\" is required".to_owned())
    })?;
    let account_id = account_id.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"account_id\" is required".to_owned())
    })?;
    let text = String::from_utf8_lossy(&file_bytes).into_owned();

    // 解析放到阻塞线程，避免大文件解析拖住异步 worker。
    let parsed = tokio::task::spawn_blocking(move || -> Result<(
        ImportFormat,
        Vec<importer::ImportRow>,
        Vec<importer::ParseIssue>,
    )> {
        let format = match format.as_deref() {
            Some(value) if value.eq_ignore_ascii_case("auto") => {
                file_name
                    .as_deref()
                    .and_then(ImportFormat::from_filename)
                    .unwrap_or_else(|| importer::sniff_format(&text))
            }
            Some(value) => ImportFormat::from_str(value)?,
            None => importer::sniff_format(&text),
        };
        importer::parse(&text, format).map(|(rows, issues)| (format, rows, issues))
    })
    .await
    .map_err(|error| KokuError::InvalidInput(format!("import parse task failed: {error}")))??;

    let mut result = lock_ledger(&state, user.user_id)
        .await?
        .import_transactions(parsed.0, account_id, category_id, currency, parsed.1)?;
    // 解析阶段跳过/失败的行（缺日期、缺金额、非收支类型等）并入结果。
    let parse_failures = parsed.2.len();
    result.failed += parse_failures;
    result.issues.extend(parsed.2);
    Ok((StatusCode::CREATED, Json(ApiResponse::new(result))))
}

async fn api_upload_receipt(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ApiResponse<Receipt>>)> {
    let mut content_type: Option<String> = None;
    let mut data: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| KokuError::InvalidInput(format!("invalid multipart upload: {error}")))?
    {
        if field.name() == Some("file") {
            content_type = field.content_type().map(|value| value.to_string());
            let bytes = field.bytes().await.map_err(|error| {
                KokuError::InvalidInput(format!("could not read upload: {error}"))
            })?;
            data = Some(bytes.to_vec());
        }
    }
    let data = data.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"file\" is required".to_owned())
    })?;
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_owned());
    let receipt = lock_ledger(&state, user.user_id).await?.attach_receipt(
        transaction_id,
        content_type,
        data,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(receipt))))
}

async fn api_get_receipt(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Response> {
    let (content_type, bytes) = lock_ledger(&state, user.user_id)
        .await?
        .receipt_bytes(transaction_id)?;
    let mut response = Response::new(axum::body::Body::from(bytes));
    let header_value = HeaderValue::from_str(&content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header_value);
    Ok(response)
}

async fn api_deposits(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Deposit>>>> {
    let deposits = lock_ledger(&state, user.user_id).await?.deposits()?;
    Ok(Json(ApiResponse::new(deposits)))
}

async fn api_create_deposit(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateDepositRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Deposit>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let source = service.account(request.from_account_id)?;
    let currency = request.currency.unwrap_or_else(|| source.currency.clone());
    let deposit = service.create_deposit(
        request.from_account_id,
        request.amount,
        currency,
        request.rate,
        request.term_days,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(deposit))))
}

async fn api_settle_deposit(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(deposit_id): AxumPath<i64>,
    Json(request): Json<SettleDepositRequest>,
) -> Result<Json<ApiResponse<DepositSettlement>>> {
    let settlement = lock_ledger(&state, user.user_id)
        .await?
        .settle_deposit(deposit_id, request.to_account_id)?;
    Ok(Json(ApiResponse::new(settlement)))
}

async fn api_mark_reimbursable(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .mark_reimbursable(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_unmark_reimbursable(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .unmark_reimbursable(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_reimburse(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<ReimburseRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let expense = service.transaction(request.expense_id)?;
    let currency = request.currency.unwrap_or_else(|| expense.currency.clone());
    let income = service.reimburse(
        request.expense_id,
        request.account_id,
        request.amount,
        currency,
        request.settled_amount,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(income))))
}

async fn api_loans(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Loan>>>> {
    let loans = lock_ledger(&state, user.user_id).await?.loans()?;
    Ok(Json(ApiResponse::new(loans)))
}

async fn api_create_loan(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateLoanRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Loan>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.account(request.account_id)?;
    let currency = request.currency.unwrap_or_else(|| account.currency.clone());
    let loan = service.create_loan(
        request.loan_type,
        request.counterparty,
        currency,
        request.amount,
        request.account_id,
        request.note,
        request.due_at,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(loan))))
}

async fn api_repay_loan(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(loan_id): AxumPath<i64>,
    Json(request): Json<RepayLoanRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Loan>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let loan = service.loan(loan_id)?;
    let currency = request.currency.unwrap_or_else(|| loan.currency.clone());
    let updated = service.repay_loan(
        loan_id,
        request.account_id,
        request.amount,
        currency,
        request.settled_amount,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(updated))))
}

async fn api_reconciliations(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<ReconciliationQuery>,
) -> Result<Json<ApiResponse<Vec<Reconciliation>>>> {
    let list = lock_ledger(&state, user.user_id)
        .await?
        .reconciliations(query.account_id)?;
    Ok(Json(ApiResponse::new(list)))
}

async fn api_create_reconciliation(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateReconciliationRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Reconciliation>>)> {
    let reconciliation = lock_ledger(&state, user.user_id)
        .await?
        .create_reconciliation(
            request.account_id,
            &request.statement_date,
            request.statement_balance,
            &request.note,
        )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(reconciliation))))
}

async fn api_complete_reconciliation(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(reconciliation_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Reconciliation>>> {
    let reconciliation = lock_ledger(&state, user.user_id)
        .await?
        .complete_reconciliation(reconciliation_id)?;
    Ok(Json(ApiResponse::new(reconciliation)))
}

async fn api_cancel_reconciliation(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(reconciliation_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Reconciliation>>> {
    let reconciliation = lock_ledger(&state, user.user_id)
        .await?
        .cancel_reconciliation(reconciliation_id)?;
    Ok(Json(ApiResponse::new(reconciliation)))
}

/// 到期提醒：未来 `days` 天内到期（含已逾期）的定存与借款。
async fn api_reminders(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<RemindersQuery>,
) -> Result<Json<ApiResponse<Vec<ReminderItem>>>> {
    let days = query.days.unwrap_or(30).clamp(1, 365);
    let items = lock_ledger(&state, user.user_id)
        .await?
        .due_reminders(days)?;
    Ok(Json(ApiResponse::new(items)))
}

/// 管理员手动发送到期提醒邮件（需配置 SMTP）。
async fn api_send_reminder_digest(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    let config = mailer::MailerConfig::from_env()?.ok_or_else(|| {
        KokuError::InvalidInput("SMTP 未配置：请设置 KOKU_SMTP_HOST/FROM/TO 环境变量".to_owned())
    })?;
    let items = lock_ledger(&state, user.user_id).await?.due_reminders(30)?;
    let subject = if items.is_empty() {
        "Koku 到期提醒：暂无".to_owned()
    } else {
        format!("Koku 到期提醒（{} 项）", items.len())
    };
    let body = reminder_digest_text(&items);
    tokio::task::spawn_blocking(move || mailer::send_mail(&config, &subject, &body))
        .await
        .map_err(|error| KokuError::AuthConfiguration(format!("smtp task failed: {error}")))??;
    Ok(Json(ApiResponse::new(serde_json::json!({
        "sent": true,
        "count": items.len(),
    }))))
}

async fn api_monthly_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<MonthlySummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .transaction_currencies(year, month)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .monthly_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_cash_flow_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<CashFlowSummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .transaction_currencies(year, month)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .cash_flow_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_tag_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TagSummaryQuery>,
) -> Result<Json<ApiResponse<TagSummary>>> {
    let tags: Vec<String> = query
        .tags
        .split(',')
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect();
    if tags.is_empty() {
        return Err(KokuError::InvalidInput(
            "at least one tag is required".to_owned(),
        ));
    }
    let currencies =
        lock_ledger(&state, user.user_id)
            .await?
            .tag_currencies(&tags, query.year, query.month)?;
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id).await?.tag_summary(
        &tags,
        query.year,
        query.month,
        &display,
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_monthly_trend(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TrendQuery>,
) -> Result<Json<ApiResponse<Vec<MonthlyTrendPoint>>>> {
    let months = query.months.unwrap_or(12);
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .trend_currencies(months)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let trend = lock_ledger(&state, user.user_id)
        .await?
        .monthly_trend(months, &display)?;
    Ok(Json(ApiResponse::new(trend)))
}

async fn api_balance_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<ApiResponse<BalanceSummary>>> {
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .balance_currencies()?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .balance_summary(&display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 年度汇总：`?year=`（缺省当前年）与 `?currency=`（缺省 CNY）。
async fn api_yearly_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<YearlySummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .yearly_currencies(year)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .yearly_summary(year, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 滚动平均：`?months=`（趋势月数，默认 12，上限 120）、
/// `?window=`（平均窗口，默认 3，上限 120）、`?currency=`（默认 CNY）。
async fn api_rolling_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TrendQueryWithWindow>,
) -> Result<Json<ApiResponse<RollingSummary>>> {
    let months = query.months.unwrap_or(12);
    let window = query.window.unwrap_or(3);
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .trend_currencies(months)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .rolling_summary(months, window, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 确保「账本中出现的各币种 → 显示币种」的折算汇率都可用：缓存缺失或超龄的
/// 现场拉取并缓存；拉取失败时报错（前端会提示具体缺失的币种对，可重试）。
async fn ensure_summary_rates(
    state: &AppState,
    user_id: i64,
    display: &str,
    currencies: Vec<String>,
) -> Result<()> {
    let today = Utc::now().date_naive();
    let missing: Vec<(String, String)> = {
        let service = lock_ledger(state, user_id).await?;
        let mut missing = Vec::new();
        for currency in currencies {
            if currency.eq_ignore_ascii_case(display) {
                continue;
            }
            if service
                .conversion_rate(&currency, display, today)?
                .is_none()
            {
                missing.push((currency, display.to_owned()));
            }
        }
        missing
    };
    for (from, to) in missing {
        match state.rates.fetch(&from, &to).await {
            Ok(quote) => {
                lock_ledger(state, user_id).await?.store_rate(&quote)?;
            }
            Err(error) => {
                return Err(KokuError::InvalidInput(format!(
                    "exchange rate unavailable for {from}->{to}: {error}"
                )));
            }
        }
    }
    Ok(())
}

/// 汇率提示：同币种直接返回 1；跨币种优先用当天/近几天的本地缓存，
/// 未命中则拉取 Frankfurter 并缓存；数据源不可达时回退到旧缓存（标记 stale）。
async fn api_rate_hint(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<RateQuery>,
) -> Result<Json<ApiResponse<RateQuote>>> {
    let from = normalize_currency(query.from)?;
    let to = normalize_currency(query.to)?;
    if from == to {
        return Ok(Json(ApiResponse::new(RateQuote {
            from,
            to,
            rate: Decimal::ONE,
            date: Utc::now().date_naive().to_string(),
            source: "identity".to_owned(),
            stale: false,
        })));
    }

    let today = Utc::now().date_naive();
    let mut stale_fallback: Option<RateQuote> = None;
    if let Some(cached) = lock_ledger(&state, user.user_id)
        .await?
        .latest_rate(&from, &to)?
    {
        if rate_is_fresh(&cached.date, today) {
            return Ok(Json(ApiResponse::new(cached)));
        }
        stale_fallback = Some(cached);
    }
    match state.rates.fetch(&from, &to).await {
        Ok(quote) => {
            lock_ledger(&state, user.user_id)
                .await?
                .store_rate(&quote)?;
            Ok(Json(ApiResponse::new(quote)))
        }
        Err(error) => {
            tracing::warn!(target: "rates", error = %error, "rate fetch failed; falling back to stale cache");
            if let Some(mut stale) = stale_fallback {
                stale.stale = true;
                return Ok(Json(ApiResponse::new(stale)));
            }
            Err(error)
        }
    }
}

pub fn api_router(state: AppState, allowed_origin: Option<HeaderValue>) -> Router {
    let protected = Router::new()
        .route("/api/accounts", get(api_accounts).post(api_create_account))
        .route("/api/accounts/{account_id}", patch(api_update_account))
        .route(
            "/api/accounts/{account_id}/adjust-balance",
            post(api_adjust_balance),
        )
        .route(
            "/api/categories",
            get(api_categories).post(api_create_category),
        )
        .route("/api/categories/{category_id}", delete(api_delete_category))
        .route("/api/tags", get(api_tags))
        .route("/api/budgets", get(api_budgets))
        .route("/api/budgets/rollover", post(api_rollover_budgets))
        .route(
            "/api/budgets/{category_id}",
            put(api_set_budget).delete(api_clear_budget),
        )
        .route(
            "/api/recurring",
            get(api_recurring_rules).post(api_create_recurring),
        )
        .route("/api/recurring/run", post(api_run_recurring))
        .route("/api/recurring/{rule_id}", delete(api_delete_recurring))
        .route("/api/holdings", get(api_holdings))
        .route("/api/holdings/refresh", post(api_refresh_holdings))
        .route("/api/holdings/buy", post(api_buy_stock))
        .route("/api/holdings/sell", post(api_sell_stock))
        .route(
            "/api/holdings/{holding_id}/price",
            put(api_set_holding_price),
        )
        .route(
            "/api/holdings/{holding_id}/refresh",
            post(api_refresh_holding),
        )
        .route(
            "/api/transactions",
            get(api_transactions).post(api_create_transaction),
        )
        .route("/api/transactions/export", get(api_export_transactions))
        .route(
            "/api/transactions/import",
            post(api_import_transactions).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/api/transfers", post(api_create_transfer))
        .route(
            "/api/transactions/{transaction_id}/void",
            post(api_void_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/restore",
            post(api_restore_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}",
            delete(api_delete_transaction).patch(api_update_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/reimbursable",
            post(api_mark_reimbursable).delete(api_unmark_reimbursable),
        )
        .route(
            "/api/transactions/{transaction_id}/receipt",
            post(api_upload_receipt)
                .get(api_get_receipt)
                .layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route("/api/reimbursements", post(api_reimburse))
        .route("/api/deposits", get(api_deposits).post(api_create_deposit))
        .route(
            "/api/deposits/{deposit_id}/settle",
            post(api_settle_deposit),
        )
        .route("/api/loans", get(api_loans).post(api_create_loan))
        .route("/api/loans/{loan_id}/repay", post(api_repay_loan))
        .route(
            "/api/reconciliations",
            get(api_reconciliations).post(api_create_reconciliation),
        )
        .route(
            "/api/reconciliations/{reconciliation_id}/complete",
            post(api_complete_reconciliation),
        )
        .route(
            "/api/reconciliations/{reconciliation_id}/cancel",
            post(api_cancel_reconciliation),
        )
        .route("/api/summary/monthly", get(api_monthly_summary))
        .route("/api/summary/by-tag", get(api_tag_summary))
        .route("/api/summary/cash-flow", get(api_cash_flow_summary))
        .route("/api/summary/trend", get(api_monthly_trend))
        .route("/api/summary/yearly", get(api_yearly_summary))
        .route("/api/summary/rolling", get(api_rolling_summary))
        .route("/api/summary/balance", get(api_balance_summary))
        .route("/api/reminders", get(api_reminders))
        .route("/api/rates", get(api_rate_hint))
        .route("/api/auth/session", get(api_auth_session))
        .route("/api/auth/password", post(api_change_password))
        .route("/api/auth/totp/setup", post(api_totp_setup))
        .route("/api/auth/totp/enable", post(api_totp_enable))
        .route("/api/auth/totp/disable", post(api_totp_disable))
        .route("/api/users", get(api_users).post(api_create_user))
        .route(
            "/api/users/{user_id}/password",
            post(api_reset_user_password),
        )
        .route("/api/users/{user_id}/enabled", post(api_set_user_enabled))
        .route("/api/users/{user_id}", delete(api_delete_user))
        .route("/api/admin/backups", get(api_list_backups))
        .route("/api/admin/backup", post(api_create_backup))
        .route(
            "/api/admin/backups/{backup_id}/download",
            get(api_download_backup),
        )
        .route(
            "/api/admin/backups/{backup_id}/restore",
            post(api_restore_backup),
        )
        .route("/api/admin/reminders/send", post(api_send_reminder_digest))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let router = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/totp", post(api_totp_verify))
        .route("/api/auth/logout", post(api_logout))
        .merge(protected)
        .with_state(state.clone());

    let router = match allowed_origin {
        Some(origin) => router.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([header::CONTENT_TYPE])
                .allow_credentials(true),
        ),
        None => router,
    };
    // 通用限流：除健康检查外，所有 /api 请求按客户端计数（防 Cookie 泄露后被刷）。
    let router = router.layer(middleware::from_fn_with_state(state, rate_limit));
    // 请求级 tracing（方法/路径/状态码/耗时），配合 tracing_subscriber 输出。
    router.layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::{csv_field, neutralize_formula};

    #[test]
    fn csv_field_escapes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn neutralize_formula_prefixes_spreadsheet_formula_leads() {
        assert_eq!(neutralize_formula("plain"), "plain");
        assert_eq!(neutralize_formula("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(neutralize_formula("+1+1"), "'+1+1");
        assert_eq!(neutralize_formula("-1+1"), "'-1+1");
        assert_eq!(neutralize_formula("@cmd"), "'@cmd");
        // 非开头的 =+-@ 不受影响。
        assert_eq!(neutralize_formula("abc=1"), "abc=1");
    }
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
        };
        fn assert_send<F: std::future::Future + Send>(_: F) {}
        assert_send(lock_ledger(&state, 1));
    }
}
