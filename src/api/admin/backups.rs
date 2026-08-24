//! 备份/恢复 API（仅管理员）：本地备份、下载、恢复与 R2 异地备份。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::backup::{self, BackupMeta};
use crate::error::{KokuError, Result};
use crate::r2::R2Client;
use crate::service::BookkeepingService;

use super::super::state::{lock_auth, ApiResponse, AppState, AuthenticatedUser};

/// 列出全部备份（管理员）。
#[utoipa::path(get, path = "/api/admin/backups", tag = "administration", responses((status = 200, description = "List backups")))]
async fn api_list_backups(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<BackupMeta>>>> {
    user.require_admin()?;
    let backups = backup::list_backups(&state.db_path)?;
    Ok(Json(ApiResponse::new(backups)))
}

/// 手动创建一份备份（管理员）。
#[utoipa::path(post, path = "/api/admin/backup", tag = "administration", responses((status = 201, description = "Create a backup")))]
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
    let mut meta = {
        let _maintenance = state.maintenance.write().await;
        backup::create_backup(&state.db_path, &state.ledger_dir, keep)?
    };
    // 配置了 R2 时自动上传本次备份，并清理超出保留份数的旧对象。
    if let Some(r2) = &state.r2 {
        upload_backup_to_r2(r2, &mut meta, &state).await?;
        crate::r2::prune_old_objects(r2, &state.db_path, keep).await;
    }
    tracing::info!(target: "auth", "admin {} created backup {}", user.username, meta.id);
    Ok((StatusCode::CREATED, Json(ApiResponse::new(meta))))
}

/// 把本地备份 zip 上传到 R2，成功后把对象键写回 `meta.r2_key`。
async fn upload_backup_to_r2(r2: &R2Client, meta: &mut BackupMeta, state: &AppState) -> Result<()> {
    let dir = backup::backup_dir(&state.db_path);
    let path = dir.join(&meta.filename);
    let bytes = std::fs::read(&path)
        .map_err(|error| KokuError::InvalidInput(format!("backup file missing: {error}")))?;
    let key = r2.object_key(&meta.filename);
    r2.put_object(&key, &bytes, "application/zip").await?;
    meta.r2_key = Some(key);
    tracing::info!(target: "koku", backup = %meta.id, "uploaded backup to R2");
    Ok(())
}

/// R2 状态（管理员）：是否启用、桶/前缀、最近一次备份的上传状态。
#[utoipa::path(get, path = "/api/admin/r2/status", tag = "administration", responses((status = 200, description = "Get R2 backup status")))]
async fn api_r2_status(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    let Some(r2) = &state.r2 else {
        return Ok(Json(ApiResponse::new(serde_json::json!({
            "enabled": false,
        }))));
    };
    // 取最近一个本地备份，HEAD 检查其在 R2 上是否存在。
    let newest = backup::list_backups(&state.db_path)?.into_iter().next();
    let mut last_uploaded: Option<serde_json::Value> = None;
    if let Some(meta) = &newest {
        let key = r2.object_key(&meta.filename);
        match r2.head_object(&key).await {
            Ok(Some(size)) => {
                last_uploaded = Some(serde_json::json!({
                    "backup_id": meta.id,
                    "size_bytes": size,
                }))
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(target: "koku", error = %error, "r2 head check failed");
            }
        }
    }
    Ok(Json(ApiResponse::new(serde_json::json!({
        "enabled": true,
        "bucket": r2.config.bucket,
        "prefix": r2.config.prefix,
        "endpoint": r2.endpoint,
        "last_uploaded": last_uploaded,
    }))))
}

/// 把某个本地备份补传到 R2（管理员）。
#[utoipa::path(post, path = "/api/admin/r2/upload/{backup_id}", tag = "administration", params(("backup_id" = String, Path)), responses((status = 200, description = "Upload a backup to R2")))]
async fn api_r2_upload(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Json<ApiResponse<BackupMeta>>> {
    user.require_admin()?;
    let r2 = state.r2.as_ref().ok_or_else(|| {
        KokuError::InvalidInput("R2 未配置：请设置 KOKU_R2_* 环境变量".to_owned())
    })?;
    let mut meta = backup::list_backups(&state.db_path)?
        .into_iter()
        .find(|meta| meta.id == backup_id)
        .ok_or_else(|| KokuError::NotFound {
            entity: "backup",
            id: backup_id.parse().unwrap_or(0),
        })?;
    upload_backup_to_r2(r2, &mut meta, &state).await?;
    Ok(Json(ApiResponse::new(meta)))
}

/// 从 R2 删除某备份对象（管理员；不影响本地备份）。
#[utoipa::path(post, path = "/api/admin/r2/delete/{backup_id}", tag = "administration", params(("backup_id" = String, Path)), responses((status = 200, description = "Delete an R2 backup")))]
async fn api_r2_delete(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    let r2 = state.r2.as_ref().ok_or_else(|| {
        KokuError::InvalidInput("R2 未配置：请设置 KOKU_R2_* 环境变量".to_owned())
    })?;
    let meta = backup::list_backups(&state.db_path)?
        .into_iter()
        .find(|meta| meta.id == backup_id)
        .ok_or_else(|| KokuError::NotFound {
            entity: "backup",
            id: backup_id.parse().unwrap_or(0),
        })?;
    let key = r2.object_key(&meta.filename);
    r2.delete_object(&key).await?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "deleted": true, "key": key }),
    )))
}

/// 从 R2 恢复某备份（管理员）：下载 zip 到本地备份目录后执行恢复，
/// 覆盖共享库与全部账本文件（所有会话失效）。
#[utoipa::path(post, path = "/api/admin/r2/restore/{backup_id}", tag = "administration", params(("backup_id" = String, Path)), responses((status = 200, description = "Restore an R2 backup")))]
async fn api_r2_restore(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    let r2 = state.r2.as_ref().ok_or_else(|| {
        KokuError::InvalidInput("R2 未配置：请设置 KOKU_R2_* 环境变量".to_owned())
    })?;
    let meta = backup::list_backups(&state.db_path)?
        .into_iter()
        .find(|meta| meta.id == backup_id)
        .ok_or_else(|| KokuError::NotFound {
            entity: "backup",
            id: backup_id.parse().unwrap_or(0),
        })?;
    let key = r2.object_key(&meta.filename);
    let bytes = r2.get_object(&key).await?;
    let dir = backup::backup_dir(&state.db_path);
    std::fs::write(dir.join(&meta.filename), bytes)?;
    // 与本地恢复走同一逻辑：覆盖文件、重开共享库、清空账本缓存。
    restore_under_maintenance(&state, &backup_id).await?;
    tracing::info!(target: "auth", "admin {} restored backup {} from R2", user.username, backup_id);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "restored": true, "source": "r2" }),
    )))
}

/// 下载备份 zip（管理员）。
#[utoipa::path(get, path = "/api/admin/backups/{backup_id}/download", tag = "administration", params(("backup_id" = String, Path)), responses((status = 200, description = "Download a backup")))]
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
#[utoipa::path(post, path = "/api/admin/backups/{backup_id}/restore", tag = "administration", params(("backup_id" = String, Path)), responses((status = 200, description = "Restore a backup")))]
async fn api_restore_backup(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(backup_id): AxumPath<String>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    user.require_admin()?;
    restore_under_maintenance(&state, &backup_id).await?;
    tracing::info!(target: "auth", "admin {} restored backup {}", user.username, backup_id);
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "restored": true }),
    )))
}

/// 在独占维护窗口内恢复文件并丢弃全部旧连接。普通请求被中间件读锁阻塞，
/// 因此不存在恢复后旧 ledger 连接继续写入已替换 inode 的窗口。
async fn restore_under_maintenance(state: &AppState, backup_id: &str) -> Result<()> {
    let _maintenance = state.maintenance.write().await;
    backup::restore_backup(&state.db_path, &state.ledger_dir, backup_id)?;
    state
        .ledgers
        .lock()
        .map_err(|_| KokuError::InvalidInput("ledger cache lock was poisoned".to_owned()))?
        .clear();
    let mut guard = lock_auth(state)?;
    *guard = BookkeepingService::open(&state.db_path)?;
    Ok(())
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
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
        .route("/api/admin/r2/status", get(api_r2_status))
        .route("/api/admin/r2/upload/{backup_id}", post(api_r2_upload))
        .route("/api/admin/r2/delete/{backup_id}", post(api_r2_delete))
        .route("/api/admin/r2/restore/{backup_id}", post(api_r2_restore))
}

api_doc!(
    BackupsApi: api_list_backups,
    api_create_backup,
    api_r2_status,
    api_r2_upload,
    api_r2_delete,
    api_r2_restore,
    api_download_backup,
    api_restore_backup,
);
