use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    #[error("JSON serialization error: {0}")]
    JsonSerialization(#[from] serde_json::Error),
    #[error("Unauthorized: {0}")] // New error
    Unauthorized(String),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            WebError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            WebError::JsonSerialization(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("JSON error: {}", err),
            ),
            WebError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()), // New mapping
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}

pub type Result<T, E = WebError> = std::result::Result<T, E>;
