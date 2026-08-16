//! REST API：请求/响应 DTO、鉴权中间件、处理器与路由。

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{ConnectInfo, Extension, Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{session_cookie, session_token, AuthConfig};
use crate::domain::{
    Account, AccountType, BalanceSummary, Budget, CashFlowSummary, Category, CategoryKind,
    DepositSettlement, Loan, LoanType, MonthlySummary, MonthlyTrendPoint, RateQuote,
    RecurrenceFrequency, RecurringRule, Transaction, TransactionKind,
};
use crate::error::{KokuError, Result};
use crate::rates::{rate_is_fresh, RateClient};
use crate::service::{normalize_currency, BookkeepingService};
use crate::throttle::LoginThrottle;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Mutex<BookkeepingService>>,
    pub auth: Arc<AuthConfig>,
    pub login_throttle: Arc<Mutex<LoginThrottle>>,
    pub rates: Arc<RateClient>,
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
    /// 信用额度（仅信用账户）
    credit_limit: Option<Decimal>,
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
    /// 与 `month` 成对出现时，只返回该自然月的流水。
    year: Option<i32>,
    month: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct TrendQuery {
    /// 返回最近多少个月，默认 12，上限 120（由 service 校验）。
    months: Option<u32>,
    currency: Option<String>,
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
struct UpdateTransactionRequest {
    note: Option<String>,
    occurred_at: Option<DateTime<Utc>>,
    category_id: Option<i64>,
    amount: Option<Decimal>,
    account_id: Option<i64>,
    settled_amount: Option<Decimal>,
}

#[derive(Debug, Deserialize)]
struct AdjustBalanceRequest {
    /// 带符号增量：正数增加余额、负数减少余额
    amount: Decimal,
    #[serde(default)]
    note: String,
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

#[derive(Debug, Deserialize)]
struct RateQuery {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
struct BudgetQuery {
    year: i32,
    month: u32,
}

#[derive(Debug, Deserialize)]
struct SetBudgetRequest {
    limit_amount: Decimal,
}

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
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response> {
    let key = LoginThrottle::client_key(&headers, Some(remote.ip()));
    // 限流检查：同一来源在窗口内失败次数已达上限时直接 429（不执行 bcrypt）。
    let locked = state
        .login_throttle
        .lock()
        .map_err(|_| KokuError::InvalidInput("login throttle lock was poisoned".to_owned()))?
        .record(&key, false)
        .is_err();
    if locked {
        tracing::warn!(target: "auth", "login blocked by rate limit from {key}");
        return Err(KokuError::RateLimited);
    }

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
        tracing::warn!(target: "auth", "failed login attempt from {key}");
        return Err(KokuError::InvalidCredentials);
    }
    // 登录成功：清除该来源的失败计数。
    if let Ok(mut throttle) = state.login_throttle.lock() {
        let _ = throttle.record(&key, true);
    }
    tracing::info!(target: "auth", "login succeeded from {key}");

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
    let mut service = lock_service(&state)?;
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
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<UpdateAccountRequest>,
) -> Result<Json<ApiResponse<Account>>> {
    let mut service = lock_service(&state)?;
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
    State(state): State<AppState>,
    AxumPath(account_id): AxumPath<i64>,
    Json(request): Json<AdjustBalanceRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction =
        lock_service(&state)?.adjust_balance(account_id, request.amount, request.note)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
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

async fn api_budgets(
    State(state): State<AppState>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Vec<Budget>>>> {
    let budgets = lock_service(&state)?.budgets(query.year, query.month)?;
    Ok(Json(ApiResponse::new(budgets)))
}

async fn api_set_budget(
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
    Json(request): Json<SetBudgetRequest>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget =
        lock_service(&state)?.set_budget(category_id, query.year, query.month, request.limit_amount)?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_clear_budget(
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget = lock_service(&state)?.clear_budget(category_id, query.year, query.month)?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_recurring_rules(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<RecurringRule>>>> {
    let rules = lock_service(&state)?.recurring_rules()?;
    Ok(Json(ApiResponse::new(rules)))
}

async fn api_create_recurring(
    State(state): State<AppState>,
    Json(request): Json<CreateRecurringRequest>,
) -> Result<(StatusCode, Json<ApiResponse<RecurringRule>>)> {
    let rule = lock_service(&state)?.create_recurring_rule(
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
    State(state): State<AppState>,
    AxumPath(rule_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<RecurringRule>>> {
    let rule = lock_service(&state)?.delete_recurring_rule(rule_id)?;
    Ok(Json(ApiResponse::new(rule)))
}

async fn api_run_recurring(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let generated = lock_service(&state)?.run_recurring()?;
    Ok(Json(ApiResponse::new(generated)))
}

async fn api_transactions(
    State(state): State<AppState>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let limit = query.limit.unwrap_or(500);
    let offset = query.offset.unwrap_or(0);
    let service = lock_service(&state)?;
    let transactions = match (query.year, query.month) {
        (Some(year), Some(month)) => service.transactions_in_month(year, month, limit, offset)?,
        (None, None) => service.transactions(limit, offset)?,
        _ => {
            return Err(KokuError::InvalidInput(
                "year and month must be provided together".to_owned(),
            ))
        }
    };
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
        TransactionKind::Adjustment => {
            return Err(KokuError::InvalidInput(
                "use /api/accounts/{id}/adjust-balance to adjust a balance".to_owned(),
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

async fn api_update_transaction(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    Json(request): Json<UpdateTransactionRequest>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_service(&state)?.update_transaction(
        transaction_id,
        request.note,
        request.occurred_at,
        request.category_id,
        request.amount,
        request.account_id,
        request.settled_amount,
    )?;
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

async fn api_unmark_reimbursable(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_service(&state)?.unmark_reimbursable(transaction_id)?;
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
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_service(&state)?.transaction_currencies(year, month)?;
    ensure_summary_rates(&state, &display, currencies).await?;
    let summary = lock_service(&state)?.monthly_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_cash_flow_summary(
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<CashFlowSummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_service(&state)?.transaction_currencies(year, month)?;
    ensure_summary_rates(&state, &display, currencies).await?;
    let summary = lock_service(&state)?.cash_flow_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_monthly_trend(
    State(state): State<AppState>,
    Query(query): Query<TrendQuery>,
) -> Result<Json<ApiResponse<Vec<MonthlyTrendPoint>>>> {
    let months = query.months.unwrap_or(12);
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_service(&state)?.trend_currencies(months)?;
    ensure_summary_rates(&state, &display, currencies).await?;
    let trend = lock_service(&state)?.monthly_trend(months, &display)?;
    Ok(Json(ApiResponse::new(trend)))
}

async fn api_balance_summary(
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<ApiResponse<BalanceSummary>>> {
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_service(&state)?.balance_currencies()?;
    ensure_summary_rates(&state, &display, currencies).await?;
    let summary = lock_service(&state)?.balance_summary(&display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 确保「账本中出现的各币种 → 显示币种」的折算汇率都可用：缓存缺失或超龄的
/// 现场拉取并缓存；拉取失败时报错（前端会提示具体缺失的币种对，可重试）。
async fn ensure_summary_rates(
    state: &AppState,
    display: &str,
    currencies: Vec<String>,
) -> Result<()> {
    let today = Utc::now().date_naive();
    let missing: Vec<(String, String)> = {
        let service = lock_service(state)?;
        let mut missing = Vec::new();
        for currency in currencies {
            if currency.eq_ignore_ascii_case(display) {
                continue;
            }
            if service
                .conversion_rate(&currency, display, today)?
                .is_none()
            {
                missing.push((currency, display.to_owned()));
            }
        }
        missing
    };
    for (from, to) in missing {
        match state.rates.fetch(&from, &to).await {
            Ok(quote) => {
                lock_service(state)?.store_rate(&quote)?;
            }
            Err(error) => {
                return Err(KokuError::InvalidInput(format!(
                    "exchange rate unavailable for {from}->{to}: {error}"
                )));
            }
        }
    }
    Ok(())
}

/// 汇率提示：同币种直接返回 1；跨币种优先用当天/近几天的本地缓存，
/// 未命中则拉取 Frankfurter 并缓存；数据源不可达时回退到旧缓存（标记 stale）。
async fn api_rate_hint(
    State(state): State<AppState>,
    Query(query): Query<RateQuery>,
) -> Result<Json<ApiResponse<RateQuote>>> {
    let from = normalize_currency(query.from)?;
    let to = normalize_currency(query.to)?;
    if from == to {
        return Ok(Json(ApiResponse::new(RateQuote {
            from,
            to,
            rate: Decimal::ONE,
            date: Utc::now().date_naive().to_string(),
            source: "identity".to_owned(),
            stale: false,
        })));
    }

    let today = Utc::now().date_naive();
    let mut stale_fallback: Option<RateQuote> = None;
    if let Some(cached) = lock_service(&state)?.latest_rate(&from, &to)? {
        if rate_is_fresh(&cached.date, today) {
            return Ok(Json(ApiResponse::new(cached)));
        }
        stale_fallback = Some(cached);
    }
    match state.rates.fetch(&from, &to).await {
        Ok(quote) => {
            lock_service(&state)?.store_rate(&quote)?;
            Ok(Json(ApiResponse::new(quote)))
        }
        Err(error) => {
            tracing::warn!(target: "rates", error = %error, "rate fetch failed; falling back to stale cache");
            if let Some(mut stale) = stale_fallback {
                stale.stale = true;
                return Ok(Json(ApiResponse::new(stale)));
            }
            Err(error)
        }
    }
}

pub fn api_router(state: AppState, allowed_origin: Option<HeaderValue>) -> Router {
    let protected = Router::new()
        .route("/api/accounts", get(api_accounts).post(api_create_account))
        .route("/api/accounts/{account_id}", patch(api_update_account))
        .route(
            "/api/accounts/{account_id}/adjust-balance",
            post(api_adjust_balance),
        )
        .route(
            "/api/categories",
            get(api_categories).post(api_create_category),
        )
        .route("/api/categories/{category_id}", delete(api_delete_category))
        .route("/api/budgets", get(api_budgets))
        .route(
            "/api/budgets/{category_id}",
            put(api_set_budget).delete(api_clear_budget),
        )
        .route("/api/recurring", get(api_recurring_rules).post(api_create_recurring))
        .route("/api/recurring/run", post(api_run_recurring))
        .route("/api/recurring/{rule_id}", delete(api_delete_recurring))
        .route(
            "/api/transactions",
            get(api_transactions).post(api_create_transaction),
        )
        .route("/api/transfers", post(api_create_transfer))
        .route(
            "/api/transactions/{transaction_id}",
            delete(api_void_transaction).patch(api_update_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/reimbursable",
            post(api_mark_reimbursable).delete(api_unmark_reimbursable),
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
        .route("/api/summary/trend", get(api_monthly_trend))
        .route("/api/summary/balance", get(api_balance_summary))
        .route("/api/rates", get(api_rate_hint))
        .route("/api/auth/session", get(api_auth_session))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let router = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/auth/login", post(api_login))
        .route("/api/auth/logout", post(api_logout))
        .merge(protected)
        .with_state(state);

    let router = match allowed_origin {
        Some(origin) => router.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([header::CONTENT_TYPE])
                .allow_credentials(true),
        ),
        None => router,
    };
    // 请求级 tracing（方法/路径/状态码/耗时），配合 tracing_subscriber 输出。
    router.layer(TraceLayer::new_for_http())
}
