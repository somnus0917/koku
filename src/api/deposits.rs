//! 定存 API：创建定期存款与到期结算。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Deposit, DepositSettlement};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct CreateDepositRequest {
    from_account_id: i64,
    amount: Decimal,
    currency: Option<String>,
    /// 利率（百分比，如 2.10 = 2.10%）
    rate: Decimal,
    term_days: u32,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct SettleDepositRequest {
    to_account_id: i64,
}

async fn api_deposits(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Deposit>>>> {
    let deposits = lock_ledger(&state, user.user_id).await?.deposits()?;
    Ok(Json(ApiResponse::new(deposits)))
}

async fn api_create_deposit(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateDepositRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Deposit>>)> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    let source = service.account(request.from_account_id)?;
    let currency = request.currency.unwrap_or_else(|| source.currency.clone());
    let deposit = service.create_deposit(
        request.from_account_id,
        request.amount,
        currency,
        request.rate,
        request.term_days,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(deposit))))
}

async fn api_settle_deposit(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(deposit_id): AxumPath<i64>,
    Json(request): Json<SettleDepositRequest>,
) -> Result<Json<ApiResponse<DepositSettlement>>> {
    let settlement = lock_ledger(&state, user.user_id)
        .await?
        .settle_deposit(deposit_id, request.to_account_id)?;
    Ok(Json(ApiResponse::new(settlement)))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/deposits", get(api_deposits).post(api_create_deposit))
        .route(
            "/api/deposits/{deposit_id}/settle",
            post(api_settle_deposit),
        )
}
