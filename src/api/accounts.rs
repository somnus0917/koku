//! 账户 API：账户 CRUD 与余额调整。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Account, AccountType, Transaction};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    name: String,
    account_type: AccountType,
    currency: String,
    opening_balance: Decimal,
    /// 信用额度（仅信用账户）
    credit_limit: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
struct UpdateAccountRequest {
    name: Option<String>,
    account_type: Option<AccountType>,
    currency: Option<String>,
    #[serde(default)]
    credit_limit: Option<Option<Decimal>>,
}

#[derive(Debug, Deserialize)]
struct AdjustBalanceRequest {
    /// 带符号增量：正数增加余额、负数减少余额
    amount: Decimal,
    #[serde(default)]
    note: String,
}

async fn api_accounts(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Account>>>> {
    let accounts = lock_ledger(&state, user.user_id).await?.accounts()?;
    Ok(Json(ApiResponse::new(accounts)))
}

async fn api_create_account(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Account>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.create_account(
        request.name,
        request.account_type,
        request.currency,
        request.opening_balance,
    )?;
    let account = match request.credit_limit {
        Some(limit) => service.set_credit_limit(account.id, Some(limit))?,
        None => account,
    };
    Ok((StatusCode::CREATED, Json(ApiResponse::new(account))))
}

async fn api_update_account(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<ApiResponse<Account>>> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let account = service.update_account(
        account_id,
        request.name,
        request.account_type,
        request.currency,
    )?;
    let account = match request.credit_limit {
        Some(limit) => service.set_credit_limit(account.id, limit)?,
        None => account,
    };
    Ok(Json(ApiResponse::new(account)))
}

async fn api_adjust_balance(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<AdjustBalanceRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.adjust_balance(
        account_id,
        request.amount,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/accounts", get(api_accounts).post(api_create_account))
        .route("/api/accounts/{account_id}", patch(api_update_account))
        .route(
            "/api/accounts/{account_id}/adjust-balance",
            post(api_adjust_balance),
        )
}
