use axum::{
    Router,
    routing::{any, get, post},
};
use http::HeaderValue;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use crate::config::ServerConfig;
use crate::error::Result as AppResult; // This is fine, AppResult is from crate::error
use crate::state::AppState;

pub mod error; // This makes src/web/error.rs a submodule
pub mod handlers;
pub mod ws;

// Correctly re-export from the submodule `error` (which is src/web/error.rs)
pub use self::error::{Result as WebResult, WebError}; // Use `self::error`

pub async fn run_server(app_state: AppState, server_config: ServerConfig) -> AppResult<()> {
    // ... (rest of the function is fine)
    let cors_origins_result: Result<Vec<HeaderValue>, _> = server_config
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse()
                .map_err(|e| format!("Invalid CORS origin '{origin}': {e}"))
        })
        .collect();

    let cors_origins = cors_origins_result.unwrap_or_else(|e| {
        tracing::error!("CORS config error: {}. Defaulting to restrictive.", e);
        vec![]
    });

    let cors = if !cors_origins.is_empty() {
        CorsLayer::new()
            .allow_methods(vec![http::Method::GET, http::Method::POST])
            .allow_origin(cors_origins)
            .allow_credentials(true)
            .allow_headers(vec![
                http::header::CONTENT_TYPE,
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
            ])
    } else {
        tracing::warn!("No valid CORS origins configured. Applying restrictive CORS policy.");
        CorsLayer::new()
    };

    let app = Router::new()
        .route("/api/create-lobby", post(handlers::create_lobby_handler))
        .route("/api/refresh-words", get(handlers::refresh_words_handler))
        .route("/ws", any(ws::ws_handler))
        .with_state(app_state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], server_config.port));
    tracing::info!("Listening on {}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app)
        .await
        .map_err(Into::into)
}
