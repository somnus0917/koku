//! REST API：路由装配与全局中间件。
//!
//! 各业务领域模块（accounts / transactions / ...）各自暴露
//! `pub(super) fn router() -> Router<AppState>`，本模块只负责：
//! 1. 声明子模块
//! 2. 导出 [`AppState`]
//! 3. 构建总 Router（合并各领域路由）
//! 4. 安装全局 middleware（鉴权 / CORS / 限流 / Trace）

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Method};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::session_token;
use crate::error::KokuError;
use crate::ratelimit::rate_limit;

macro_rules! api_doc {
    ($name:ident: $($path:ident),+ $(,)?) => {
        #[derive(utoipa::OpenApi)]
        #[openapi(paths($($path),+))]
        pub(in crate::api) struct $name;
    };
}

mod accounts;
mod activity;
mod admin;
mod auth;
mod budgets;
mod categories;
mod deposits;
mod holdings;
mod import_export;
mod loans;
mod payees;
mod planning;
mod rates;
mod reconciliations;
mod recurring;
mod refunds;
mod reimbursements;
mod reminders;
mod rules;
mod state;
mod summaries;
mod transactions;

pub use state::AppState;
pub(crate) use state::{lock_auth, lock_ledger};

use state::AuthenticatedUser;

/// 备份和恢复自身需要取得维护写锁，不能在本中间件持读锁，否则会自锁。
fn maintenance_operation(path: &str) -> bool {
    path == "/api/admin/backup"
        || path.ends_with("/restore")
        || path.starts_with("/api/admin/r2/restore/")
}

async fn maintenance_gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if maintenance_operation(request.uri().path()) {
        return next.run(request).await;
    }
    let _guard = state.maintenance.read().await;
    next.run(request).await
}

async fn require_auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let Some(token) = session_token(request.headers()) else {
        return KokuError::Unauthorized.into_response();
    };
    let user = match lock_auth(&state).and_then(|service| {
        service
            .authenticated_user(&token)?
            .ok_or(KokuError::Unauthorized)
    }) {
        Ok(user) => user,
        Err(error) => return error.into_response(),
    };
    request.extensions_mut().insert(AuthenticatedUser {
        user_id: user.id,
        username: user.username,
        role: user.role,
        totp_enabled: user.totp_enabled,
    });
    next.run(request).await
}

#[utoipa::path(
    get,
    path = "/api/health",
    tag = "system",
    responses((status = 200, description = "API is healthy"))
)]
async fn api_health() -> Json<state::ApiResponse<serde_json::Value>> {
    Json(state::ApiResponse::new(serde_json::json!({
        "status": "ok",
        "service": "koku-api"
    })))
}

#[derive(OpenApi)]
#[openapi(paths(api_health))]
struct ApiDoc;

fn openapi() -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    for module in [
        accounts::AccountsApi::openapi(),
        activity::ActivityApi::openapi(),
        admin::backups::BackupsApi::openapi(),
        admin::users::UsersApi::openapi(),
        auth::AuthApi::openapi(),
        budgets::BudgetsApi::openapi(),
        categories::CategoriesApi::openapi(),
        deposits::DepositsApi::openapi(),
        holdings::HoldingsApi::openapi(),
        import_export::ImportExportApi::openapi(),
        loans::LoansApi::openapi(),
        payees::PayeesApi::openapi(),
        planning::PlanningApi::openapi(),
        rates::RatesApi::openapi(),
        reconciliations::ReconciliationsApi::openapi(),
        recurring::RecurringApi::openapi(),
        refunds::RefundsApi::openapi(),
        reimbursements::ReimbursementsApi::openapi(),
        reminders::RemindersApi::openapi(),
        rules::RulesApi::openapi(),
        summaries::SummariesApi::openapi(),
        transactions::TransactionsApi::openapi(),
    ] {
        document.merge(module);
    }
    document
}

pub fn api_router(state: AppState, allowed_origin: Option<HeaderValue>) -> Router {
    let protected = Router::new()
        .merge(accounts::router())
        .merge(activity::router())
        .merge(categories::router())
        .merge(transactions::router())
        .merge(import_export::router())
        .merge(deposits::router())
        .merge(loans::router())
        .merge(payees::router())
        .merge(planning::router())
        .merge(reimbursements::router())
        .merge(refunds::router())
        .merge(budgets::router())
        .merge(recurring::router())
        .merge(rules::router())
        .merge(holdings::router())
        .merge(reconciliations::router())
        .merge(reminders::router())
        .merge(summaries::router())
        .merge(rates::router())
        .merge(auth::router())
        .merge(admin::router())
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let router = Router::new()
        .route("/api/health", get(api_health))
        .merge(auth::public_router())
        .merge(protected)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi()))
        .with_state(state.clone());

    let router = match allowed_origin {
        Some(origin) => router.layer(
            CorsLayer::new()
                .allow_origin(origin)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers([header::CONTENT_TYPE])
                .allow_credentials(true),
        ),
        None => router,
    };
    // 通用限流：除健康检查外，所有 /api 请求按客户端计数（防 Cookie 泄露后被刷）。
    let router = router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            maintenance_gate,
        ))
        .layer(middleware::from_fn_with_state(state, rate_limit));
    // 把业务处理中的意外 panic 转成 500，避免连接被直接中断或进程退出。
    let router = router.layer(CatchPanicLayer::new());
    // 请求级 tracing（方法/路径/状态码/耗时），配合 tracing_subscriber 输出。
    router.layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::{maintenance_operation, openapi};

    #[test]
    fn maintenance_gate_skips_only_operations_that_take_the_write_lock() {
        assert!(maintenance_operation("/api/admin/backup"));
        assert!(maintenance_operation(
            "/api/admin/backups/20260821-010203/restore"
        ));
        assert!(maintenance_operation(
            "/api/admin/r2/restore/20260821-010203"
        ));
        assert!(!maintenance_operation("/api/admin/backups"));
        assert!(!maintenance_operation("/api/transactions"));
    }

    #[test]
    fn openapi_registers_public_and_protected_routes() {
        let document = openapi();
        assert_eq!(document.paths.paths.len(), 88);
        assert!(document.paths.paths.contains_key("/api/health"));
        assert!(document.paths.paths.contains_key("/api/auth/login"));
        assert!(document.paths.paths.contains_key("/api/transactions"));
        assert!(document.paths.paths.contains_key("/api/admin/backups"));
    }
}
