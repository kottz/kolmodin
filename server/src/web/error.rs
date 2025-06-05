use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Lobby not found: {0}")]
    LobbyNotFound(Uuid),
    #[error("Internal server error: {0}")]
    InternalServerError(String),
    #[error("WebSocket handshake error: {0}")]
    WebSocketHandshake(String),
    #[error("JSON serialization error: {0}")]
    JsonSerialization(#[from] serde_json::Error),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            WebError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            WebError::LobbyNotFound(id) => {
                (StatusCode::NOT_FOUND, format!("Lobby {} not found", id))
            }
            WebError::InternalServerError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            WebError::WebSocketHandshake(msg) => {
                (StatusCode::BAD_REQUEST, format!("WebSocket error: {}", msg))
            }
            WebError::JsonSerialization(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("JSON error: {}", err),
            ),
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}

pub type Result<T, E = WebError> = std::result::Result<T, E>;
