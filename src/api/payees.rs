//! Payee API：搜索/列出商户（自动补全）与学习数据清理。

use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::Payee;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct PayeeQuery {
    /// 名称包含查询词；缺省返回全部。
    q: Option<String>,
    /// 返回条数上限（默认 50，上限 200）。
    limit: Option<u32>,
}

async fn api_payees(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<PayeeQuery>,
) -> Result<Json<ApiResponse<Vec<Payee>>>> {
    let payees = lock_ledger(&state, user.user_id)
        .await?
        .search_payees(query.q.as_deref().unwrap_or(""), query.limit.unwrap_or(50))?;
    Ok(Json(ApiResponse::new(payees)))
}

/// 清除自动分类学习数据（merchant_aliases 与 payee_category_stats）。
/// 不删除 Payee、不删除交易、不修改已有交易分类。
async fn api_clear_payee_learning(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    lock_ledger(&state, user.user_id)
        .await?
        .clear_payee_learning()?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "cleared": true }),
    )))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/payees", get(api_payees))
        .route("/api/payees/clear-learning", post(api_clear_payee_learning))
}
