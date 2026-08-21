//! 持仓 API：持仓列表、买卖股票、市价刷新与手动改价。

use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::{Holding, Transaction};
use crate::error::{KokuError, Result};
use crate::quotes::Quote;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct TradeRequest {
    account_id: i64,
    symbol: String,
    shares: Decimal,
    price: Decimal,
    #[serde(default)]
    fee: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct SetPriceRequest {
    price: Decimal,
}

#[derive(Debug, Deserialize)]
struct QuoteQuery {
    symbol: String,
}

/// 市价新鲜度阈值（小时）；`KOKU_QUOTE_TTL_HOURS` 可覆盖，默认 24。
fn quote_ttl_hours() -> i64 {
    std::env::var("KOKU_QUOTE_TTL_HOURS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(24)
}

async fn api_holdings(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Holding>>>> {
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    Ok(Json(ApiResponse::new(holdings)))
}

async fn api_buy_stock(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<TradeRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.buy_stock(
        request.account_id,
        request.symbol,
        request.shares,
        request.price,
        request.fee,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_sell_stock(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<TradeRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_ledger(&state, user.user_id).await?.sell_stock(
        request.account_id,
        request.symbol,
        request.shares,
        request.price,
        request.fee,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

/// 供买入表单按证券代码查询参考价；失败时前端仍可由用户输入手动价格。
async fn api_quote(
    Extension(_user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<QuoteQuery>,
) -> Result<Json<ApiResponse<Quote>>> {
    let symbol = query.symbol.trim();
    if symbol.is_empty() {
        return Err(KokuError::InvalidInput(
            "stock symbol cannot be empty".to_owned(),
        ));
    }
    Ok(Json(ApiResponse::new(state.quotes.fetch(symbol).await?)))
}

async fn api_set_holding_price(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(holding_id): AxumPath<i64>,
    Json(request): Json<SetPriceRequest>,
) -> Result<Json<ApiResponse<Holding>>> {
    let holding = lock_ledger(&state, user.user_id)
        .await?
        .set_holding_price(holding_id, request.price)?;
    Ok(Json(ApiResponse::new(holding)))
}

/// 刷新全部过期（或缺失）市价：并发拉取 Stooq 后批量写回，返回逐标的明细。
async fn api_refresh_holdings(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    let ttl_hours = quote_ttl_hours();
    let now = Utc::now();
    let stale: Vec<(i64, String)> = holdings
        .iter()
        .filter(|holding| {
            holding
                .updated_at
                .map(|updated| now.signed_duration_since(updated).num_hours() >= ttl_hours)
                .unwrap_or(true)
        })
        .map(|holding| (holding.id, holding.symbol.clone()))
        .collect();

    // 并发拉取行情（持仓数量通常很少，JoinSet 足够）。
    let mut join_set = tokio::task::JoinSet::new();
    for (holding_id, symbol) in stale {
        let client = state.quotes.clone();
        join_set.spawn(async move {
            let result = client.fetch(&symbol).await;
            (holding_id, symbol, result)
        });
    }
    let mut refreshed = 0_usize;
    let mut failed: Vec<serde_json::Value> = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        let (holding_id, symbol, result) = joined.map_err(|error| {
            KokuError::InvalidInput(format!("quote fetch task failed: {error}"))
        })?;
        match result {
            Ok(quote) => {
                lock_ledger(&state, user.user_id)
                    .await?
                    .set_holding_quote(holding_id, &quote)?;
                refreshed += 1;
            }
            Err(error) => failed.push(serde_json::json!({
                "symbol": symbol,
                "error": error.to_string(),
            })),
        }
    }
    let holdings = lock_ledger(&state, user.user_id).await?.holdings()?;
    Ok(Json(ApiResponse::new(serde_json::json!({
        "refreshed": refreshed,
        "failed": failed,
        "holdings": holdings,
    }))))
}

/// 刷新单只持仓市价（忽略 TTL，强制拉取）。
async fn api_refresh_holding(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(holding_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Holding>>> {
    let symbol = {
        let service = lock_ledger(&state, user.user_id).await?;
        service.holding(holding_id)?.symbol
    };
    let quote = state.quotes.fetch(&symbol).await?;
    let holding = lock_ledger(&state, user.user_id)
        .await?
        .set_holding_quote(holding_id, &quote)?;
    Ok(Json(ApiResponse::new(holding)))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/holdings", get(api_holdings))
        .route("/api/holdings/quote", get(api_quote))
        .route("/api/holdings/refresh", post(api_refresh_holdings))
        .route("/api/holdings/buy", post(api_buy_stock))
        .route("/api/holdings/sell", post(api_sell_stock))
        .route(
            "/api/holdings/{holding_id}/price",
            put(api_set_holding_price),
        )
        .route(
            "/api/holdings/{holding_id}/refresh",
            post(api_refresh_holding),
        )
}
