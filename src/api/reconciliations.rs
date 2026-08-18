//! 对账 API：创建、完成与取消对账。

use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::Reconciliation;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

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

pub(super) fn router() -> Router<AppState> {
    Router::new()
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
}
