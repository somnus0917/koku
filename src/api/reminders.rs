//! 到期提醒 API：查询到期项与管理员手动发送提醒邮件。

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::{KokuError, Result};
use crate::mailer;
use crate::service::{reminder_digest_text, ReminderItem};

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct RemindersQuery {
    /// 未来多少天内到期（默认 30，上限 365）。
    days: Option<i64>,
}

/// 到期提醒：未来 `days` 天内到期（含已逾期）的定存、借款与信用卡账单。
async fn api_reminders(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<RemindersQuery>,
) -> Result<Json<ApiResponse<Vec<ReminderItem>>>> {
    let days = query.days.unwrap_or(30).clamp(1, 365);
    let mut ledger = lock_ledger(&state, user.user_id).await?;
    let items = ledger.due_reminders(days)?;
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
    let mut ledger = lock_ledger(&state, user.user_id).await?;
    let items = ledger.due_reminders(30)?;
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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/reminders", get(api_reminders))
        .route("/api/admin/reminders/send", post(api_send_reminder_digest))
}
