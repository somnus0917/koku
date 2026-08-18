//! 汇总 API：月度/年度/现金流/标签/趋势/滚动/余额汇总。

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Datelike, Utc};
use serde::Deserialize;

use crate::domain::{
    BalanceSummary, CashFlowSummary, MonthlySummary, MonthlyTrendPoint, RollingSummary, TagSummary,
    YearlySummary,
};
use crate::error::{KokuError, Result};
use crate::service::normalize_currency;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct MonthlyQuery {
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagSummaryQuery {
    /// 逗号分隔的标签名（AND 语义：交易须同时带有全部标签）。
    tags: String,
    /// 缺省时统计全部历史；year/month 必须同时给出或同时缺省。
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TrendQuery {
    /// 返回最近多少个月，默认 12，上限 120（由 service 校验）。
    months: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TrendQueryWithWindow {
    /// 返回最近多少个月，默认 12，上限 120（由 service 校验）。
    months: Option<u32>,
    /// 滚动平均窗口（月），默认 3，上限 120（由 service 校验）。
    window: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BalanceQuery {
    currency: Option<String>,
}

async fn api_monthly_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<MonthlySummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .transaction_currencies(year, month)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .monthly_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_cash_flow_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<CashFlowSummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let month = query.month.unwrap_or_else(|| now.month());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .transaction_currencies(year, month)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .cash_flow_summary(year, month, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_tag_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TagSummaryQuery>,
) -> Result<Json<ApiResponse<TagSummary>>> {
    let tags: Vec<String> = query
        .tags
        .split(',')
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect();
    if tags.is_empty() {
        return Err(KokuError::InvalidInput(
            "at least one tag is required".to_owned(),
        ));
    }
    let currencies =
        lock_ledger(&state, user.user_id)
            .await?
            .tag_currencies(&tags, query.year, query.month)?;
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id).await?.tag_summary(
        &tags,
        query.year,
        query.month,
        &display,
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_monthly_trend(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TrendQuery>,
) -> Result<Json<ApiResponse<Vec<MonthlyTrendPoint>>>> {
    let months = query.months.unwrap_or(12);
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .trend_currencies(months)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let trend = lock_ledger(&state, user.user_id)
        .await?
        .monthly_trend(months, &display)?;
    Ok(Json(ApiResponse::new(trend)))
}

async fn api_balance_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<ApiResponse<BalanceSummary>>> {
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .balance_currencies()?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .balance_summary(&display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 年度汇总：`?year=`（缺省当前年）与 `?currency=`（缺省 CNY）。
async fn api_yearly_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<YearlySummary>>> {
    let now = Utc::now();
    let year = query.year.unwrap_or_else(|| now.year());
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .yearly_currencies(year)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .yearly_summary(year, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 滚动平均：`?months=`（趋势月数，默认 12，上限 120）、
/// `?window=`（平均窗口，默认 3，上限 120）、`?currency=`（默认 CNY）。
async fn api_rolling_summary(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<TrendQueryWithWindow>,
) -> Result<Json<ApiResponse<RollingSummary>>> {
    let months = query.months.unwrap_or(12);
    let window = query.window.unwrap_or(3);
    let display = normalize_currency(query.currency.unwrap_or_else(|| "CNY".to_owned()))?;
    let currencies = lock_ledger(&state, user.user_id)
        .await?
        .trend_currencies(months)?;
    ensure_summary_rates(&state, user.user_id, &display, currencies).await?;
    let summary = lock_ledger(&state, user.user_id)
        .await?
        .rolling_summary(months, window, &display)?;
    Ok(Json(ApiResponse::new(summary)))
}

/// 确保「账本中出现的各币种 → 显示币种」的折算汇率都可用：缓存缺失或超龄的
/// 现场拉取并缓存；拉取失败时报错（前端会提示具体缺失的币种对，可重试）。
async fn ensure_summary_rates(
    state: &AppState,
    user_id: i64,
    display: &str,
    currencies: Vec<String>,
) -> Result<()> {
    let today = Utc::now().date_naive();
    let missing: Vec<(String, String)> = {
        let service = lock_ledger(state, user_id).await?;
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
                lock_ledger(state, user_id).await?.store_rate(&quote)?;
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

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/summary/monthly", get(api_monthly_summary))
        .route("/api/summary/by-tag", get(api_tag_summary))
        .route("/api/summary/cash-flow", get(api_cash_flow_summary))
        .route("/api/summary/trend", get(api_monthly_trend))
        .route("/api/summary/yearly", get(api_yearly_summary))
        .route("/api/summary/rolling", get(api_rolling_summary))
        .route("/api/summary/balance", get(api_balance_summary))
}
