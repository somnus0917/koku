//! 汇率提示 API：同币种恒等、优先本地缓存、兜底现场拉取。

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::RateQuote;
use crate::error::Result;
use crate::rates::rate_is_fresh;
use crate::service::normalize_currency;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct RateQuery {
    from: String,
    to: String,
}

/// 汇率提示：同币种直接返回 1；跨币种优先用当天/近几天的本地缓存，
/// 未命中则拉取 Frankfurter 并缓存；数据源不可达时回退到旧缓存（标记 stale）。
#[utoipa::path(get, path = "/api/rates", tag = "rates", responses((status = 200, description = "Get an exchange-rate hint")))]
async fn api_rate_hint(
    Extension(user): Extension<AuthenticatedUser>,
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
    if let Some(cached) = lock_ledger(&state, user.user_id)
        .await?
        .latest_rate(&from, &to)?
    {
        if rate_is_fresh(&cached.date, today) {
            return Ok(Json(ApiResponse::new(cached)));
        }
        stale_fallback = Some(cached);
    }
    match state.rates.fetch(&from, &to).await {
        Ok(quote) => {
            lock_ledger(&state, user.user_id)
                .await?
                .store_rate(&quote)?;
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

pub(super) fn router() -> Router<AppState> {
    Router::new().route("/api/rates", get(api_rate_hint))
}

api_doc!(RatesApi: api_rate_hint);
