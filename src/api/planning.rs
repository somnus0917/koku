use super::state::{lock_ledger, ApiResponse, AppState, AuthenticatedUser};
use crate::domain::{Bill, ImportProfile, SavingsGoal};
use crate::error::Result;
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
#[derive(Deserialize)]
struct Profile {
    name: String,
    format: String,
    account_id: Option<i64>,
    category_id: Option<i64>,
    currency: Option<String>,
}
#[derive(Deserialize)]
struct BillInput {
    name: String,
    account_id: i64,
    category_id: i64,
    amount: Decimal,
    due_day: u32,
    #[serde(default = "active")]
    active: bool,
    #[serde(default)]
    note: String,
}
fn active() -> bool {
    true
}
#[derive(Deserialize)]
struct Goal {
    name: String,
    account_id: Option<i64>,
    target_amount: Decimal,
    #[serde(default)]
    current_amount: Decimal,
    target_date: Option<NaiveDate>,
}
#[utoipa::path(get, path = "/api/import-profiles", tag = "planning", responses((status = 200, description = "List import profiles")))]
async fn profiles(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
) -> Result<Json<ApiResponse<Vec<ImportProfile>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.import_profiles()?,
    )))
}
#[utoipa::path(post, path = "/api/import-profiles", tag = "planning", responses((status = 201, description = "Create an import profile")))]
async fn profile_create(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    Json(v): Json<Profile>,
) -> Result<(StatusCode, Json<ApiResponse<ImportProfile>>)> {
    let p = lock_ledger(&s, u.user_id).await?.save_import_profile(
        None,
        v.name,
        v.format,
        v.account_id,
        v.category_id,
        v.currency,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(p))))
}
#[utoipa::path(put, path = "/api/import-profiles/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 200, description = "Update an import profile")))]
async fn profile_update(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(v): Json<Profile>,
) -> Result<Json<ApiResponse<ImportProfile>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.save_import_profile(
            Some(id),
            v.name,
            v.format,
            v.account_id,
            v.category_id,
            v.currency,
        )?,
    )))
}
#[utoipa::path(delete, path = "/api/import-profiles/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 204, description = "Delete an import profile")))]
async fn profile_delete(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&s, u.user_id)
        .await?
        .delete_import_profile(id)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get, path = "/api/bills", tag = "planning", responses((status = 200, description = "List bills")))]
async fn bills(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Bill>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.bills()?,
    )))
}
#[utoipa::path(post, path = "/api/bills", tag = "planning", responses((status = 201, description = "Create a bill")))]
async fn bill_create(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    Json(v): Json<BillInput>,
) -> Result<(StatusCode, Json<ApiResponse<Bill>>)> {
    let b = lock_ledger(&s, u.user_id).await?.save_bill(
        None,
        v.name,
        v.account_id,
        v.category_id,
        v.amount,
        v.due_day,
        v.active,
        v.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(b))))
}
#[utoipa::path(put, path = "/api/bills/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 200, description = "Update a bill")))]
async fn bill_update(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(v): Json<BillInput>,
) -> Result<Json<ApiResponse<Bill>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.save_bill(
            Some(id),
            v.name,
            v.account_id,
            v.category_id,
            v.amount,
            v.due_day,
            v.active,
            v.note,
        )?,
    )))
}
#[utoipa::path(delete, path = "/api/bills/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 204, description = "Delete a bill")))]
async fn bill_delete(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&s, u.user_id).await?.delete_bill(id)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(get, path = "/api/savings-goals", tag = "planning", responses((status = 200, description = "List savings goals")))]
async fn goals(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
) -> Result<Json<ApiResponse<Vec<SavingsGoal>>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.savings_goals()?,
    )))
}
#[utoipa::path(post, path = "/api/savings-goals", tag = "planning", responses((status = 201, description = "Create a savings goal")))]
async fn goal_create(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    Json(v): Json<Goal>,
) -> Result<(StatusCode, Json<ApiResponse<SavingsGoal>>)> {
    let g = lock_ledger(&s, u.user_id).await?.save_savings_goal(
        None,
        v.name,
        v.account_id,
        v.target_amount,
        v.current_amount,
        v.target_date,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(g))))
}
#[utoipa::path(put, path = "/api/savings-goals/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 200, description = "Update a savings goal")))]
async fn goal_update(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
    Json(v): Json<Goal>,
) -> Result<Json<ApiResponse<SavingsGoal>>> {
    Ok(Json(ApiResponse::new(
        lock_ledger(&s, u.user_id).await?.save_savings_goal(
            Some(id),
            v.name,
            v.account_id,
            v.target_amount,
            v.current_amount,
            v.target_date,
        )?,
    )))
}
#[utoipa::path(delete, path = "/api/savings-goals/{id}", tag = "planning", params(("id" = i64, Path)), responses((status = 204, description = "Delete a savings goal")))]
async fn goal_delete(
    Extension(u): Extension<AuthenticatedUser>,
    State(s): State<AppState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode> {
    lock_ledger(&s, u.user_id).await?.delete_savings_goal(id)?;
    Ok(StatusCode::NO_CONTENT)
}
pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/import-profiles", get(profiles).post(profile_create))
        .route(
            "/api/import-profiles/{id}",
            put(profile_update).delete(profile_delete),
        )
        .route("/api/bills", get(bills).post(bill_create))
        .route("/api/bills/{id}", put(bill_update).delete(bill_delete))
        .route("/api/savings-goals", get(goals).post(goal_create))
        .route(
            "/api/savings-goals/{id}",
            put(goal_update).delete(goal_delete),
        )
}

api_doc!(
    PlanningApi: profiles,
    profile_create,
    profile_update,
    profile_delete,
    bills,
    bill_create,
    bill_update,
    bill_delete,
    goals,
    goal_create,
    goal_update,
    goal_delete,
);
