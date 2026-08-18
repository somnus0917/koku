//! 流水 API：交易 CRUD、转账、作废/恢复与小票附件。

use axum::extract::{DefaultBodyLimit, Extension, Multipart, Path as AxumPath, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Receipt, SplitInput, Transaction, TransactionKind, TransactionSplit};
use crate::error::{KokuError, Result};

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

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
    #[serde(default)]
    tag_names: Vec<String>,
    /// 商户/收款方名称（可选）：按名称查找/创建并参与分类学习。
    #[serde(default)]
    payee_name: Option<String>,
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
struct UpdateTransactionRequest {
    note: Option<String>,
    occurred_at: Option<DateTime<Utc>>,
    category_id: Option<i64>,
    amount: Option<Decimal>,
    account_id: Option<i64>,
    settled_amount: Option<Decimal>,
    /// 提供时整体替换标签；不提供则保持不变。
    tag_names: Option<Vec<String>>,
    /// 商户/收款方：提供非空值按名称设置并学习；提供空串清除；不提供保持不变。
    #[serde(default)]
    payee_name: Option<String>,
    /// 拆分分类（可选）：不提供保持不变；空数组清除拆分；提供数组则与父交易
    /// 字段在同一个事务内整体替换（各拆分金额之和须等于最终父交易金额）。
    #[serde(default)]
    splits: Option<Vec<SplitInput>>,
}

async fn api_transactions(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TransactionQuery>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let limit = query.limit.unwrap_or(500);
    let offset = query.offset.unwrap_or(0);
    let service = lock_ledger(&state, user.user_id).await?;
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
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    let mut service = lock_ledger(&state, user.user_id).await?;
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
        TransactionKind::Trade => {
            return Err(KokuError::InvalidInput(
                "use /api/holdings/buy or /api/holdings/sell for trades".to_owned(),
            ))
        }
        TransactionKind::Deposit => {
            return Err(KokuError::InvalidInput(
                "use /api/deposits for fixed deposits".to_owned(),
            ))
        }
    };
    if !request.tag_names.is_empty() {
        service.set_transaction_tags(transaction.id, request.tag_names)?;
    }
    // 手动记账带 Payee：设置商户；仅当用户显式提供 Payee 时确认学习样本
    // （没有 Payee 本就无法形成 Payee → Category 样本）。
    if let Some(payee_name) = request.payee_name {
        if !payee_name.trim().is_empty() {
            service.set_transaction_payee(transaction.id, Some(&payee_name))?;
        }
        service.confirm_transaction_learning(transaction.id)?;
    }
    let transaction = service.transaction(transaction.id)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_create_transfer(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateTransferRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.record_transfer(
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
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .void_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_restore_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_ledger(&state, user.user_id)
        .await?
        .restore_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_delete_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&state, user.user_id)
        .await?
        .delete_transaction(transaction_id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_update_transaction(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    Json(request): Json<UpdateTransactionRequest>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let mut service = lock_ledger(&state, user.user_id).await?;
    // 只有用户显式修改 Payee 或 Category 才视为人工确认（机器自动识别的导入
    // 交易仅改 note/amount/时间/账户等无关字段不应反向强化学习）。
    let confirm_learning = request.payee_name.is_some() || request.category_id.is_some();
    // 父交易字段 + Payee + 标签 + 拆分 + 学习确认在同一个 SQLite 事务内原子
    // 提交：任一步失败全部回滚，绝不出现「部分字段已保存」的中间状态。
    let transaction = service.update_transaction_edit(
        transaction_id,
        request.note,
        request.occurred_at,
        request.category_id,
        request.amount,
        request.account_id,
        request.settled_amount,
        request.splits.as_deref(),
        request.payee_name.as_deref(),
        request.tag_names.as_deref(),
        confirm_learning,
    )?;
    Ok(Json(ApiResponse::new(transaction)))
}

/// 列出交易的拆分分类（无拆分为空数组）。
async fn api_list_transaction_splits(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Vec<TransactionSplit>>>> {
    let splits = lock_ledger(&state, user.user_id)
        .await?
        .list_transaction_splits(transaction_id)?;
    Ok(Json(ApiResponse::new(splits)))
}

/// 清除交易的拆分分类（恢复父交易分类统计）。
async fn api_clear_transaction_splits(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    lock_ledger(&state, user.user_id)
        .await?
        .clear_transaction_splits(transaction_id)?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "cleared": true }),
    )))
}

/// 原子替换交易的拆分分类（金额总和须等于父交易金额）。
async fn api_set_transaction_splits(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    Json(request): Json<Vec<SplitInput>>,
) -> Result<Json<ApiResponse<Vec<TransactionSplit>>>> {
    let splits = lock_ledger(&state, user.user_id)
        .await?
        .set_transaction_splits(transaction_id, &request)?;
    Ok(Json(ApiResponse::new(splits)))
}

async fn api_upload_receipt(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ApiResponse<Receipt>>)> {
    let mut content_type: Option<String> = None;
    let mut data: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| KokuError::InvalidInput(format!("invalid multipart upload: {error}")))?
    {
        if field.name() == Some("file") {
            content_type = field.content_type().map(|value| value.to_string());
            let bytes = field.bytes().await.map_err(|error| {
                KokuError::InvalidInput(format!("could not read upload: {error}"))
            })?;
            data = Some(bytes.to_vec());
        }
    }
    let data = data.ok_or_else(|| {
        KokuError::InvalidInput("multipart field \"file\" is required".to_owned())
    })?;
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_owned());
    let receipt = lock_ledger(&state, user.user_id).await?.attach_receipt(
        transaction_id,
        content_type,
        data,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(receipt))))
}

async fn api_get_receipt(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Response> {
    let (content_type, bytes) = lock_ledger(&state, user.user_id)
        .await?
        .receipt_bytes(transaction_id)?;
    let mut response = Response::new(axum::body::Body::from(bytes));
    let header_value = HeaderValue::from_str(&content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header_value);
    Ok(response)
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/transactions",
            get(api_transactions).post(api_create_transaction),
        )
        .route("/api/transfers", post(api_create_transfer))
        .route(
            "/api/transactions/{transaction_id}/void",
            post(api_void_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/restore",
            post(api_restore_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}",
            delete(api_delete_transaction).patch(api_update_transaction),
        )
        .route(
            "/api/transactions/{transaction_id}/splits",
            get(api_list_transaction_splits)
                .put(api_set_transaction_splits)
                .delete(api_clear_transaction_splits),
        )
        .route(
            "/api/transactions/{transaction_id}/receipt",
            post(api_upload_receipt)
                .get(api_get_receipt)
                .layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
}
