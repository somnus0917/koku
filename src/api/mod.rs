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
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::session_token;
use crate::error::KokuError;
use crate::ratelimit::rate_limit;

mod accounts;
mod admin;
mod auth;
mod budgets;
mod categories;
mod deposits;
mod holdings;
mod import_export;
mod loans;
mod payees;
mod rates;
mod reconciliations;
mod recurring;
mod reimbursements;
mod reminders;
mod state;
mod summaries;
mod transactions;

pub use state::AppState;

use state::{lock_auth, AuthenticatedUser};

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

async fn api_health() -> Json<state::ApiResponse<serde_json::Value>> {
    Json(state::ApiResponse::new(serde_json::json!({
        "status": "ok",
        "service": "koku-api"
    })))
}

pub fn api_router(state: AppState, allowed_origin: Option<HeaderValue>) -> Router {
    let protected = Router::new()
        .merge(accounts::router())
        .merge(categories::router())
        .merge(transactions::router())
        .merge(import_export::router())
        .merge(deposits::router())
        .merge(loans::router())
        .merge(payees::router())
        .merge(reimbursements::router())
        .merge(budgets::router())
        .merge(recurring::router())
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
    let router = router.layer(middleware::from_fn_with_state(state, rate_limit));
    // 请求级 tracing（方法/路径/状态码/耗时），配合 tracing_subscriber 输出。
    router.layer(TraceLayer::new_for_http())
}
