//! 用户可见活动轨迹 API。

use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::ActivityEvent;
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct ActivityQuery { limit: Option<u32> }

async fn list(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<ApiResponse<Vec<ActivityEvent>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&state, user.user_id).await?.activity_events(query.limit.unwrap_or(80))?,
    )))
}

pub(super) fn router() -> Router<AppState> {
    Router::new().route("/api/activity", get(list))
}
