//! 周期交易 API：规则 CRUD 与到期批量执行。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{RecurrenceFrequency, RecurringRule, Transaction, TransactionKind};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/recurring",
            get(api_recurring_rules).post(api_create_recurring),
        )
        .route("/api/recurring/run", post(api_run_recurring))
        .route("/api/recurring/{rule_id}", delete(api_delete_recurring))
}
