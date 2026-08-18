//! 用户管理 API（仅管理员）。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::{User, UserRole};
use crate::error::{KokuError, Result};

use super::super::auth::hash_password;
use super::super::state::{lock_auth, ApiResponse, AppState, AuthenticatedUser};

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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(api_users).post(api_create_user))
        .route(
            "/api/users/{user_id}/password",
            post(api_reset_user_password),
        )
        .route("/api/users/{user_id}/enabled", post(api_set_user_enabled))
        .route("/api/users/{user_id}", delete(api_delete_user))
}
