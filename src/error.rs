//! 统一错误类型及其 HTTP 响应映射。
//!
//! 500 类错误（数据库/IO/配置）只向客户端返回通用文案，详细信息通过 `tracing`
//! 写入日志，避免内部错误细节透出公网。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

/// 全 crate 共用的结果别名。
pub type Result<T> = std::result::Result<T, KokuError>;

#[derive(Debug, Error)]
pub enum KokuError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid decimal stored in database: {0}")]
    InvalidDecimal(#[from] rust_decimal::Error),
    #[error("{entity} with id {id} was not found")]
    NotFound { entity: &'static str, id: i64 },
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("category kind must be {expected}, but was {actual}")]
    CategoryKindMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("a transaction that has already been voided cannot be voided again")]
    AlreadyVoided,
    #[error("invalid username or password")]
    InvalidCredentials,
    #[error("authentication required")]
    Unauthorized,
    #[error("too many failed login attempts; try again later")]
    RateLimited,
    #[error("authentication configuration error: {0}")]
    AuthConfiguration(String),
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: String,
}

impl IntoResponse for KokuError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::AlreadyVoided => StatusCode::CONFLICT,
            Self::InvalidCredentials | Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidInput(_) | Self::CategoryKindMismatch { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Database(_)
            | Self::InvalidDecimal(_)
            | Self::Io(_)
            | Self::AuthConfiguration(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "internal error");
        }
        // 500 只返回通用文案，细节进日志；其余错误原样返回（本应用无敏感内部信息）。
        let message = if status.is_server_error() {
            "internal server error".to_owned()
        } else {
            self.to_string()
        };
        (status, Json(ApiErrorBody { error: message })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    #[tokio::test]
    async fn internal_errors_return_generic_message_and_log_details() {
        let response =
            KokuError::Io(std::io::Error::other("secret disk path /var/lib/koku")).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("internal server error"));
        assert!(!text.contains("secret disk path"));
    }

    #[tokio::test]
    async fn client_errors_keep_their_message() {
        let response = KokuError::InvalidInput("bad amount".to_owned()).into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("bad amount"));
    }
}
