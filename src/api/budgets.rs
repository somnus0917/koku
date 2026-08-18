//! 预算 API：按分类/月份设置、清除与滚动复制预算。

use axum::extract::{Extension, Path as AxumPath, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::Budget;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct BudgetQuery {
    year: i32,
    month: u32,
}

#[derive(Debug, Deserialize)]
struct SetBudgetRequest {
    limit_amount: Decimal,
}

async fn api_budgets(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Vec<Budget>>>> {
    let budgets = lock_ledger(&state, user.user_id)
        .await?
        .budgets(query.year, query.month)?;
    Ok(Json(ApiResponse::new(budgets)))
}

async fn api_set_budget(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
    Json(request): Json<SetBudgetRequest>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget = lock_ledger(&state, user.user_id).await?.set_budget(
        category_id,
        query.year,
        query.month,
        request.limit_amount,
    )?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_clear_budget(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
    Query(query): Query<BudgetQuery>,
) -> Result<Json<ApiResponse<Budget>>> {
    let budget = lock_ledger(&state, user.user_id).await?.clear_budget(
        category_id,
        query.year,
        query.month,
    )?;
    Ok(Json(ApiResponse::new(budget)))
}

async fn api_rollover_budgets(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let copied = lock_ledger(&state, user.user_id)
        .await?
        .rollover_budgets_once(Utc::now())?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({ "copied": copied }),
    )))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/budgets", get(api_budgets))
        .route("/api/budgets/rollover", post(api_rollover_budgets))
        .route(
            "/api/budgets/{category_id}",
            put(api_set_budget).delete(api_clear_budget),
        )
}
