//! 管理后台 API：用户管理与备份/恢复。

use axum::Router;

use super::state::AppState;

pub(super) mod backups;
pub(super) mod users;

/// 管理后台受保护路由（叠加在全局鉴权中间件之上）。
pub(super) fn router() -> Router<AppState> {
    Router::new()
        .merge(users::router())
        .merge(backups::router())
}
