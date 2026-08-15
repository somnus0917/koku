//! REST API：请求/响应 DTO、鉴权中间件、处理器与路由。

use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Extension, Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use crate::auth::{session_cookie, session_token, AuthConfig};
use crate::domain::{
    Account, AccountType, BalanceSummary, CashFlowSummary, Category, CategoryKind,
    DepositSettlement, Loan, LoanType, MonthlySummary, Transaction, TransactionKind,
};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Mutex<BookkeepingService>>,
    pub auth: Arc<AuthConfig>,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    data: T,
}

impl<T> ApiResponse<T> {
    fn new(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AuthenticatedUser {
    username: String,
}

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    name: String,
    account_type: AccountType,
    currency: String,
    opening_balance: Decimal,
}

#[derive(Debug, Deserialize)]
struct CreateCategoryRequest {
    name: String,
    kind: CategoryKind,
}

#[derive(Debug, Deserialize)]
struct CreateTransactionRequest {
    kind: TransactionKind,
    account_id: i64,
    category_id: i64,
    amount: Decimal,
    currency: Option<String>,
    settled_amount: Option<Decimal>,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct CreateTransferRequest {
    from_account_id: i64,
    to_account_id: i64,
    source_amount: Decimal,
    target_amount: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct MonthlyQuery {
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TransactionQuery {
    /// 返回条数，默认 500，上限 1000（由 service 校验）。
    limit: Option<u32>,
    /// 跳过条数，默认 0。
    offset: Option<u32>,
}

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

#[derive(Debug, Deserialize)]
struct CreateLoanRequest {
    loan_type: LoanType,
    counterparty: String,
    currency: Option<String>,
    amount: Decimal,
    account_id: i64,
    #[serde(default)]
    note: String,
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

#[derive(Debug, Deserialize)]
struct BalanceQuery {
    currency: Option<String>,
}

fn lock_service(state: &AppState) -> Result<MutexGuard<'_, BookkeepingService>> {
    state
        .service
        .lock()
        .map_err(|_| KokuError::InvalidInput("bookkeeping service lock was poisoned".to_owned()))
}

async fn require_auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let Some(token) = session_token(request.headers()) else {
        return KokuError::Unauthorized.into_response();
    };
    let username = match lock_service(&state).and_then(|service| {
        service
            .authenticated_username(&token)?
            .ok_or(KokuError::Unauthorized)
    }) {
        Ok(username) => username,
        Err(error) => return error.into_response(),
    };
    request
        .extensions_mut()
        .insert(AuthenticatedUser { username });
    next.run(request).await
}

async fn api_login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> Result<Response> {
    let password = request.password;
    let password_hash = state.auth.password_hash.clone();
    let password_matches =
        tokio::task::spawn_blocking(move || bcrypt::verify(password, &password_hash))
            .await
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("password verification task failed: {error}"))
            })?
            .map_err(|error| KokuError::AuthConfiguration(error.to_string()))?;
    if request.username != state.auth.username || !password_matches {
        return Err(KokuError::InvalidCredentials);
    }

    let token = lock_service(&state)?
        .create_auth_session(&state.auth.username, state.auth.session_ttl_seconds)?;
    let mut response = Json(ApiResponse::new(AuthenticatedUser {
        username: state.auth.username.clone(),
    }))
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(
            &token,
            state.auth.session_ttl_seconds,
            state.auth.cookie_secure,
        )?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn api_auth_session(Extension(user): Extension<AuthenticatedUser>) -> Response {
    let mut response = Json(ApiResponse::new(user)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn api_logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let Some(token) = session_token(&headers) {
        lock_service(&state)?.delete_auth_session(&token)?;
    }
    let mut response =
        Json(ApiResponse::new(serde_json::json!({ "logged_out": true }))).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie("", 0, state.auth.cookie_secure)?,
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

async fn api_health() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::new(serde_json::json!({
        "status": "ok",
        "service": "koku-api"
    })))
}

async fn api_accounts(State(state): State<AppState>) -> Result<Json<ApiResponse<Vec<Account>>>> {
    let accounts = lock_service(&state)?.accounts()?;
    Ok(Json(ApiResponse::new(accounts)))
}

async fn api_create_account(
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Account>>)> {
    let account = lock_service(&state)?.create_account(
        request.name,
        request.account_type,
        request.currency,
        request.opening_balance,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(account))))
}

async fn api_categories(State(state): State<AppState>) -> Result<Json<ApiResponse<Vec<Category>>>> {
    let categories = lock_service(&state)?.categories()?;
    Ok(Json(ApiResponse::new(categories)))
}

async fn api_create_category(
    State(state): State<AppState>,
    Json(request): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Category>>)> {
    let category = lock_service(&state)?.create_category(request.name, request.kind)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(category))))
}

async fn api_delete_category(
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Category>>> {
    let category = lock_service(&state)?.delete_category(category_id)?;
    Ok(Json(ApiResponse::new(category)))
}

async fn api_transactions(
    State(state): State<AppState>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let transactions = lock_service(&state)?
        .transactions(query.limit.unwrap_or(500), query.offset.unwrap_or(0))?;
    Ok(Json(ApiResponse::new(transactions)))
}

async fn api_create_transaction(
    State(state): State<AppState>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    let mut service = lock_service(&state)?;
    let account = service.account(request.account_id)?;
    let currency = request.currency.unwrap_or_else(|| account.currency.clone());
    let settled_amount = match request.settled_amount {
        Some(amount) => amount,
        None if currency.eq_ignore_ascii_case(&account.currency) => request.amount,
        None => {
            return Err(KokuError::InvalidInput(format!(
                "settled_amount in {} is required for a {currency} transaction",
                account.currency
            )))
        }
    };
    let transaction = match request.kind {
        TransactionKind::Expense => service.record_expense_in_currency(
            request.account_id,
            request.category_id,
            request.amount,
            currency,
            settled_amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Income => service.record_income_in_currency(
            request.account_id,
            request.category_id,
            request.amount,
            currency,
            settled_amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Transfer => {
            return Err(KokuError::InvalidInput(
                "use /api/transfers for transfer transactions".to_owned(),
            ))
        }
        TransactionKind::Loan => {
            return Err(KokuError::InvalidInput(
                "use /api/loans for loan transactions".to_owned(),
            ))
        }
    };
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_create_transfer(
    State(state): State<AppState>,
    Json(request): Json<CreateTransferRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_service(&state)?.record_transfer(
        request.from_account_id,
        request.to_account_id,
        request.source_amount,
        request.target_amount,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_void_transaction(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_service(&state)?.void_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_create_deposit(
    State(state): State<AppState>,
    Json(request): Json<CreateDepositRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Account>>)> {
    let mut service = lock_service(&state)?;
    let source = service.account(request.from_account_id)?;
    let currency = request.currency.unwrap_or_else(|| source.currency.clone());
    let deposit = service.create_fixed_deposit(
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
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<SettleDepositRequest>,
) -> Result<Json<ApiResponse<DepositSettlement>>> {
    let settlement = lock_service(&state)?.settle_deposit(account_id, request.to_account_id)?;
    Ok(Json(ApiResponse::new(settlement)))
}

async fn api_mark_reimbursable(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_service(&state)?.mark_reimbursable(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_reimburse(
    State(state): State<AppState>,
    Json(request): Json<ReimburseRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let mut service = lock_service(&state)?;
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

async fn api_loans(State(state): State<AppState>) -> Result<Json<ApiResponse<Vec<Loan>>>> {
    let loans = lock_service(&state)?.loans()?;
    Ok(Json(ApiResponse::new(loans)))
}

async fn api_create_loan(
    State(state): State<AppState>,
    Json(request): Json<CreateLoanRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Loan>>)> {
    let mut service = lock_service(&state)?;
    let account = service.account(request.account_id)?;
    let currency = request.currency.unwrap_or_else(|| account.currency.clone());
    let loan = service.create_loan(
        request.loan_type,
        request.counterparty,
        currency,
        request.amount,
        request.account_id,
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(loan))))
}

async fn api_repay_loan(
    State(state): State<AppState>,
    AxumPath(loan_id): AxumPath<i64>,
    Json(request): Json<RepayLoanRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Loan>>)> {
    let mut service = lock_service(&state)?;
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
    Ok((StatusCode::CREATED, Json(ApiResponse::new(updated))))
}

async fn api_monthly_summary(
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<MonthlySummary>>> {
    let now = Utc::now();
    let summary = lock_service(&state)?.monthly_summary(
        query.year.unwrap_or_else(|| now.year()),
        query.month.unwrap_or_else(|| now.month()),
        query.currency.as_deref().unwrap_or("CNY"),
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_cash_flow_summary(
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<CashFlowSummary>>> {
    let now = Utc::now();
    let summary = lock_service(&state)?.cash_flow_summary(
        query.year.unwrap_or_else(|| now.year()),
        query.month.unwrap_or_else(|| now.month()),
        query.currency.as_deref().unwrap_or("CNY"),
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_balance_summary(
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<ApiResponse<BalanceSummary>>> {
    let summary =
        lock_service(&state)?.balance_summary(query.currency.as_deref().unwrap_or("CNY"))?;
    Ok(Json(ApiResponse::new(summary)))
}

pub fn api_router(state: AppState, allowed_origin: Option<HeaderValue>) -> Router {
    let protected = Router::new()
        .route("/api/accounts", get(api_accounts).post(api_create_account))
        .route(
            "/api/categories",
            get(api_categories).post(api_create_category),
        )
        .route("/api/categories/{category_id}", delete(api_delete_category))
        .route(
            "/api/transactions",
            get(api_transactions).post(api_create_transaction),
        )
        .route("/api/transfers", post(api_create_transfer))
        .route(
            "/api/transactions/{transaction_id}",
            delete(api_void_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/reimbursable",
            post(api_mark_reimbursable),
        )
        .route("/api/reimbursements", post(api_reimburse))
        .route("/api/deposits", post(api_create_deposit))
        .route(
            "/api/deposits/{account_id}/settle",
            post(api_settle_deposit),
        )
        .route("/api/loans", get(api_loans).post(api_create_loan))
        .route("/api/loans/{loan_id}/repay", post(api_repay_loan))
        .route("/api/summary/monthly", get(api_monthly_summary))
        .route("/api/summary/cash-flow", get(api_cash_flow_summary))
        .route("/api/summary/balance", get(api_balance_summary))
        .route("/api/auth/session", get(api_auth_session))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let router = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/logout", post(api_logout))
        .merge(protected)
        .with_state(state);

    match allowed_origin {
        Some(origin) => router.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE])
                .allow_credentials(true),
        ),
        None => router,
    }
}
