//! 分类与标签 API：分类 CRUD 与全部标签列表。

use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::Deserialize;

use crate::domain::{Category, CategoryKind, Tag};
use crate::error::Result;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};

#[derive(Debug, Deserialize)]
struct CreateCategoryRequest {
    name: String,
    kind: CategoryKind,
    /// 用户自选图标（lucide 图标名，可空；空白视为未选择）。
    #[serde(default)]
    icon: Option<String>,
}

#[utoipa::path(get, path = "/api/categories", tag = "categories", responses((status = 200, description = "List categories")))]
async fn api_categories(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Category>>>> {
    let categories = lock_ledger(&state, user.user_id).await?.categories()?;
    Ok(Json(ApiResponse::new(categories)))
}

#[utoipa::path(post, path = "/api/categories", tag = "categories", responses((status = 201, description = "Create a category")))]
async fn api_create_category(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(request): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Category>>)> {
    let category = lock_ledger(&state, user.user_id)
        .await?
        .create_category_with_icon(request.name, request.kind, request.icon)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(category))))
}

#[utoipa::path(
    delete,
    path = "/api/categories/{category_id}",
    tag = "categories",
    params(("category_id" = i64, Path)),
    responses((status = 200, description = "Delete a category"))
)]
async fn api_delete_category(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(category_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Category>>> {
    let category = lock_ledger(&state, user.user_id)
        .await?
        .delete_category(category_id)?;
    Ok(Json(ApiResponse::new(category)))
}

#[utoipa::path(get, path = "/api/tags", tag = "categories", responses((status = 200, description = "List tags")))]
async fn api_tags(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Tag>>>> {
    let tags = lock_ledger(&state, user.user_id).await?.all_tags()?;
    Ok(Json(ApiResponse::new(tags)))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/categories",
            get(api_categories).post(api_create_category),
        )
        .route("/api/categories/{category_id}", delete(api_delete_category))
        .route("/api/tags", get(api_tags))
}

api_doc!(
    CategoriesApi: api_categories,
    api_create_category,
    api_delete_category,
    api_tags,
);
