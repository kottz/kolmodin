use axum::{
    Router,
    routing::{any, post},
};
use http::HeaderValue;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use crate::config::ServerConfig;
use crate::error::Result as AppResult;
use crate::state::AppState;

pub mod error;
pub mod handlers;
pub mod ws;

pub use error::{Result as WebResult, WebError};

pub async fn run_server(app_state: AppState, server_config: ServerConfig) -> AppResult<()> {
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
        CorsLayer::new()
    };

    let app = Router::new()
        .route("/api/create-lobby", post(handlers::create_lobby_handler))
        .route("/ws", any(ws::ws_handler))
        .with_state(app_state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], server_config.port));
    tracing::info!("Listening on {}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await?, app)
        .await
        .map_err(Into::into)
}
