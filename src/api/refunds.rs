//! 退款 API：为原支出创建一笔关联退款收入，并入账到指定账户。

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::Transaction;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct RefundRequest {
    expense_id: i64,
    account_id: i64,
    amount: Decimal,
    currency: Option<String>,
    settled_amount: Option<Decimal>,
    #[serde(default)]
    note: String,
}

#[utoipa::path(post, path = "/api/refunds", tag = "refunds", responses((status = 200, description = "Create a refund")))]
async fn api_refund(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<RefundRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let expense = service.transaction(request.expense_id)?;
    let currency = request.currency.unwrap_or_else(|| expense.currency.clone());
    let income = service.refund(
        request.expense_id,
        request.account_id,
        request.amount,
        currency,
        request.settled_amount,
        request.note,
    )?;
    service.record_activity_best_effort(
        "refund.created",
        "refund",
        income.id,
        format!("记录了退款收入：{} {}", income.amount, income.currency),
    );
    Ok((StatusCode::CREATED, Json(ApiResponse::new(income))))
}

pub(super) fn router() -> Router<AppState> {
    Router::new().route("/api/refunds", post(api_refund))
}

api_doc!(RefundsApi: api_refund);
