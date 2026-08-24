use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::Deserialize;

use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};
use crate::domain::{TransactionKind, TransactionRule, TransactionRulePreview};
use crate::error::Result;
use crate::service::rules::TransactionRuleInput;

#[derive(Debug, Deserialize)]
struct RuleRequest {
    name: String,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    #[serde(default)]
    priority: i64,
    description_contains: Option<String>,
    account_id: Option<i64>,
    kind: Option<TransactionKind>,
    min_amount: Option<Decimal>,
    max_amount: Option<Decimal>,
    category_id: Option<i64>,
    payee_name: Option<String>,
    #[serde(default)]
    tag_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApplyRuleRequest {
    transaction_ids: Vec<i64>,
}
fn enabled_by_default() -> bool {
    true
}
impl From<RuleRequest> for TransactionRuleInput {
    fn from(v: RuleRequest) -> Self {
        Self {
            name: v.name,
            enabled: v.enabled,
            priority: v.priority,
            description_contains: v.description_contains,
            account_id: v.account_id,
            kind: v.kind,
            min_amount: v.min_amount,
            max_amount: v.max_amount,
            category_id: v.category_id,
            payee_name: v.payee_name,
            tag_names: v.tag_names,
        }
    }
}
#[utoipa::path(get, path = "/api/rules", tag = "rules", responses((status = 200, description = "List transaction rules")))]
async fn list(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<TransactionRule>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&state, user.user_id)
            .await?
            .transaction_rules()?,
    )))
}
#[utoipa::path(post, path = "/api/rules", tag = "rules", responses((status = 201, description = "Create a transaction rule")))]
async fn create(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    Json(body): Json<RuleRequest>,
) -> Result<(StatusCode, Json<ApiResponse<TransactionRule>>)> {
    let rule = lock_ledger(&state, user.user_id)
        .await?
        .create_transaction_rule(body.into())?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(rule))))
}
#[utoipa::path(put, path = "/api/rules/{id}", tag = "rules", params(("id" = i64, Path)), responses((status = 200, description = "Update a transaction rule")))]
async fn update(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(body): Json<RuleRequest>,
) -> Result<Json<ApiResponse<TransactionRule>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&state, user.user_id)
            .await?
            .update_transaction_rule(id, body.into())?,
    )))
}
#[utoipa::path(delete, path = "/api/rules/{id}", tag = "rules", params(("id" = i64, Path)), responses((status = 204, description = "Delete a transaction rule")))]
async fn remove(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&state, user.user_id)
        .await?
        .delete_transaction_rule(id)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(post, path = "/api/rules/{id}/apply", tag = "rules", params(("id" = i64, Path)), responses((status = 200, description = "Apply a transaction rule")))]
async fn apply(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(body): Json<ApplyRuleRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>> {
    let changed = lock_ledger(&state, user.user_id)
        .await?
        .apply_transaction_rule_preview(id, &body.transaction_ids)?;
    Ok(Json(ApiResponse::new(
        serde_json::json!({"applied":changed}),
    )))
}
#[utoipa::path(get, path = "/api/rules/{id}/preview", tag = "rules", params(("id" = i64, Path)), responses((status = 200, description = "Preview a transaction rule")))]
async fn preview(
    Extension(user): Extension<AuthenticatedUser>,
    State(state): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Vec<TransactionRulePreview>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&state, user.user_id)
            .await?
            .preview_transaction_rule(id)?,
    )))
}
pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/rules", get(list).post(create))
        .route("/api/rules/{id}", put(update).delete(remove))
        .route("/api/rules/{id}/preview", get(preview))
        .route("/api/rules/{id}/apply", post(apply))
}

api_doc!(RulesApi: list, create, update, remove, apply, preview);
