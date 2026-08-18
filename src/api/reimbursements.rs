//! 报销 API：标记可报销、取消标记与报销入账。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::Transaction;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/transactions/{transaction_id}/reimbursable",
            post(api_mark_reimbursable).delete(api_unmark_reimbursable),
        )
        .route("/api/reimbursements", post(api_reimburse))
}
