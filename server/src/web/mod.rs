use axum::{
    Router,
    routing::{any, get, post},
};
use http::HeaderValue;
use std::{net::SocketAddr, sync::Arc};
use tokio::time::Duration as TokioDuration;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::compression::CompressionLevel;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use tracing::warn;

use crate::config::ServerConfig;
use crate::error::Result as AppResult;
use crate::state::AppState;

pub mod error;
pub mod handlers;
pub mod ws;

pub use self::error::WebError;

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
        tracing::warn!("No valid CORS origins configured. Applying restrictive CORS policy.");
        CorsLayer::new()
    };

    let governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_millisecond(500)
            .burst_size(30)
            .finish()
            .unwrap(),
    );

    let governor_limiter = governor_conf.limiter().clone();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TokioDuration::from_secs(60)).await;
            if governor_limiter.len() > 1_000_000 {
                warn!(
                    "Rate limiting storage size is large: {}",
                    governor_limiter.len()
                );
            }
            governor_limiter.retain_recent();
        }
    });

    let app = Router::new()
        .route("/api/create-lobby", post(handlers::create_lobby_handler))
        .route("/api/refresh-words", get(handlers::refresh_words_handler))
        .route("/ws", any(ws::ws_handler))
        .with_state(app_state)
        .layer(TraceLayer::new_for_http())
        .layer(
            CompressionLayer::new()
                .quality(CompressionLevel::Default)
                .gzip(true),
        )
        .layer(GovernorLayer {
            config: governor_conf,
        })
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], server_config.port));
    tracing::info!("Listening on {}", addr);

    axum::serve(
        tokio::net::TcpListener::bind(addr).await?,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(Into::into)
}
