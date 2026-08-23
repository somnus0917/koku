//! 认证 API：登录、TOTP 二阶段登录、TOTP 管理、会话与密码修改。

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Extension, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;

use crate::auth::{generate_session_token, session_cookie, session_token};
use crate::domain::AuthSession;
use crate::error::{KokuError, Result};
use crate::throttle::LoginThrottle;
use crate::totp;

use super::state::{lock_auth, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct LoginRequest {
    #[serde(alias = "username")]
    email: String,
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

/// 校验并生成密码哈希（bcrypt，代价默认）。
pub(super) async fn hash_password(password: String) -> Result<String> {
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
        .user_by_username(&request.email)?
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
            "email": user.username,
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

/// 受保护（需登录）的认证路由。
pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/session", get(api_auth_session))
        .route("/api/auth/password", post(api_change_password))
        .route("/api/auth/totp/setup", post(api_totp_setup))
        .route("/api/auth/totp/enable", post(api_totp_enable))
        .route("/api/auth/totp/disable", post(api_totp_disable))
}

/// 公开认证路由（无需登录即可访问）。
pub(super) fn public_router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/totp", post(api_totp_verify))
        .route("/api/auth/logout", post(api_logout))
}
