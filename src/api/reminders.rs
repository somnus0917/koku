//! 到期提醒 API：查询到期项与管理员手动发送提醒邮件。

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Datelike;
use serde::Deserialize;

use crate::error::{KokuError, Result};
use crate::mailer;
use crate::service::{normalize_currency, reminder_digest_text, ReminderItem};

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct RemindersQuery {
    /// 未来多少天内到期（默认 30，上限 365）。
    days: Option<i64>,
    /// 预算预警的显示币种（默认 CNY）。
    currency: Option<String>,
}

/// 合并未来到期事项和本月预算预警；API 与定时邮件共用同一口径。
pub(crate) async fn load_reminder_items(
    state: &AppState,
    user_id: i64,
    days: i64,
    currency: &str,
) -> Result<Vec<ReminderItem>> {
    let now = chrono::Utc::now();
    let currencies = lock_ledger(state, user_id)
        .await?
        .transaction_currencies(now.year(), now.month())?;
    super::summaries::ensure_summary_rates(state, user_id, currency, currencies).await?;
    let mut ledger = lock_ledger(state, user_id).await?;
    let mut items = ledger.due_reminders(days)?;
    items.extend(ledger.budget_alerts(now.year(), now.month(), currency, 90)?);
    items.sort_by_key(|item| item.due_at);
    Ok(items)
}

/// 到期提醒：未来 `days` 天内到期（含已逾期）的定存、借款与信用卡账单。
#[utoipa::path(get, path = "/api/reminders", tag = "reminders", responses((status = 200, description = "List due reminders")))]
async fn api_reminders(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<RemindersQuery>,
) -> Result<Json<ApiResponse<Vec<ReminderItem>>>> {
    let days = query.days.unwrap_or(30).clamp(1, 365);
    let currency = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let items = load_reminder_items(&state, user.user_id, days, &currency).await?;
    Ok(Json(ApiResponse::new(items)))
}

/// 手动向当前用户邮箱发送其账本的到期提醒（需配置 SMTP）。
#[utoipa::path(post, path = "/api/reminders/send", tag = "reminders", responses((status = 200, description = "Send the reminder digest")))]
async fn api_send_reminder_digest(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<RemindersQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let config = mailer::MailerConfig::from_env()?.ok_or_else(|| {
        KokuError::InvalidInput("SMTP 未配置：请设置 KOKU_SMTP_HOST/FROM 环境变量".to_owned())
    })?;
    let currency = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let items = load_reminder_items(&state, user.user_id, 30, &currency).await?;
    let subject = if items.is_empty() {
        "Koku 资金提醒：暂无".to_owned()
    } else {
        format!("Koku 资金提醒（{} 项）", items.len())
    };
    let body = reminder_digest_text(&items);
    let recipient = user.username;
    tokio::task::spawn_blocking(move || mailer::send_mail(&config, &recipient, &subject, &body))
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
        .route("/api/reminders/send", post(api_send_reminder_digest))
}

api_doc!(
    RemindersApi: api_reminders,
    api_send_reminder_digest,
);
