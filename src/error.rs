//! 统一错误类型及其 HTTP 响应映射。

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
            Self::InvalidInput(_) | Self::CategoryKindMismatch { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Database(_)
            | Self::InvalidDecimal(_)
            | Self::Io(_)
            | Self::AuthConfiguration(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ApiErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}
