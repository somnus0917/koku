//! 借款/出借 API：创建借款与还款。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Loan, LoanType};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

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
    service.record_activity("loan.created", "loan", loan.id, format!("记录了与 {} 的借款", loan.counterparty))?;
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
    service.record_activity("loan.repaid", "loan", updated.id, format!("记录了与 {} 的还款", updated.counterparty))?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(updated))))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/loans", get(api_loans).post(api_create_loan))
        .route("/api/loans/{loan_id}/repay", post(api_repay_loan))
}
