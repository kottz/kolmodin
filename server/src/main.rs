use dashmap::DashMap;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod content;
mod error;
mod game_logic;
mod lobby;
mod state;
mod twitch;
mod web;

use crate::config::load_settings;
use crate::content::GameContentCache;
use crate::error::Result as AppResult;
use crate::state::AppState;
use crate::twitch::TokenProvider;
use crate::twitch::TwitchChatManagerActorHandle;
use crate::web::run_server;

#[tracing::instrument(name = "main")]
#[tokio::main]
async fn main() -> AppResult<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{app_name}=info,tower_http=debug,{app_name}::db=debug,{app_name}::twitch=trace",
                    app_name = env!("CARGO_PKG_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Application starting...");

    let app_settings = load_settings()?;
    tracing::info!(
        config.source = "environment variables",
        "Configuration loaded successfully"
    );

    tracing::info!("Initializing core components...");

    let game_content_cache = Arc::new(GameContentCache::new(app_settings.database.clone()).await?);
    let initial_medandraord_words = game_content_cache.medandraord_words().await;
    let initial_whitelist = game_content_cache.twitch_whitelist().await;
    tracing::info!(
        words.count = initial_medandraord_words.len(),
        twitch.channels.count = initial_whitelist.len(),
        "GameContentCache initialized"
    );

    let token_provider = TokenProvider::new(Arc::new(app_settings.twitch.clone())).await?;
    tracing::info!("TokenProvider initialized");

    let twitch_chat_manager_handle =
        TwitchChatManagerActorHandle::spawn(token_provider.clone(), 32, 32);
    tracing::info!("TwitchChatManagerActor created");

    let active_lobbies = Arc::new(DashMap::new());
    let server_config_for_state = Arc::new(app_settings.server.clone());
    let shared_app_settings = Arc::new(app_settings.clone());
    let games_config = app_settings.games.clone();
    let server_config_for_run = app_settings.server.clone();

    let app_state = AppState {
        active_lobbies,
        game_content_cache,
        server_config: server_config_for_state,
        games_config,
        twitch_chat_manager: twitch_chat_manager_handle,
        app_settings: shared_app_settings,
    };

    tracing::info!(
        server.port = server_config_for_run.port,
        server.cors_origins.count = server_config_for_run.cors_origins.len(),
        "Starting HTTP server"
    );

    run_server(app_state, server_config_for_run).await?;

    tracing::info!("Application shutting down");
    Ok(())
}
