//! 账户 API：账户 CRUD、余额调整与信用卡账单摘要。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Account, AccountType, CreditCardStatement, CreditCardSummary, Transaction};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    name: String,
    account_type: AccountType,
    currency: String,
    opening_balance: Decimal,
    /// 信用额度（仅信用账户；必须 >= 0）
    #[serde(default)]
    credit_limit: Option<Decimal>,
    /// 账单日（1~31；仅信用账户）
    #[serde(default)]
    statement_day: Option<u32>,
    /// 还款日（1~31；仅信用账户）
    #[serde(default)]
    due_day: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct UpdateAccountRequest {
    name: Option<String>,
    account_type: Option<AccountType>,
    currency: Option<String>,
    #[serde(default)]
    credit_limit: Option<Option<Decimal>>,
    /// 账单日（1~31）；null 清除；不提供保持不变。
    #[serde(default)]
    statement_day: Option<Option<u32>>,
    /// 还款日（1~31）；null 清除；不提供保持不变。
    #[serde(default)]
    due_day: Option<Option<u32>>,
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
    // 名称/类型/币种/期初余额/额度/账单日/还款日在同一个 SQLite 事务内原子写入，
    // 任一步校验失败全部回滚、账户不落库。
    let account = service.create_account_edit(
        request.name,
        request.account_type,
        request.currency,
        request.opening_balance,
        request.credit_limit,
        request.statement_day,
        request.due_day,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(account))))
}

async fn api_update_account(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<ApiResponse<Account>>> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    // 名称/类型/币种/额度/账单日/还款日在同一个 SQLite 事务内原子提交，
    // 任一步校验失败全部回滚。
    let account = service.update_account_edit(
        account_id,
        request.name,
        request.account_type,
        request.currency,
        request.credit_limit,
        request.statement_day,
        request.due_day,
    )?;
    Ok(Json(ApiResponse::new(account)))
}

/// 信用卡账单摘要（额度/出账/未出账/账单与还款日）；仅对 Credit 账户有效。
async fn api_credit_card_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<CreditCardSummary>>> {
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .credit_card_summary(account_id, Utc::now())?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_credit_card_statements(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Vec<CreditCardStatement>>>> {
    let statements = lock_ledger(&state, user.user_id)
        .await?
        .credit_card_statements_history(account_id, Utc::now())?;
    Ok(Json(ApiResponse::new(statements)))
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
            "/api/accounts/{account_id}/credit-card-summary",
            get(api_credit_card_summary),
        )
        .route(
            "/api/accounts/{account_id}/credit-card-statements",
            get(api_credit_card_statements),
        )
        .route(
            "/api/accounts/{account_id}/adjust-balance",
            post(api_adjust_balance),
        )
}
