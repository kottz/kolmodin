use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod db;
mod error;
mod game_logic;
mod lobby;
mod state;
mod twitch;
mod web;

use crate::config::load_settings;
use crate::db::WordListManager;
use crate::error::Result as AppResult;
use crate::lobby::LobbyManagerHandle;
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

    let word_list_manager = Arc::new(WordListManager::new(app_settings.database.clone()).await?);
    let initial_mao_words = word_list_manager.get_med_andra_ord_words().await;
    let initial_whitelist = word_list_manager.get_twitch_whitelist().await;
    tracing::info!(
        words.count = initial_mao_words.len(),
        twitch.channels.count = initial_whitelist.len(),
        "WordListManager initialized"
    );

    let token_provider = TokenProvider::new(Arc::new(app_settings.twitch.clone())).await?;
    tracing::info!("TokenProvider initialized");

    let twitch_chat_manager_handle =
        TwitchChatManagerActorHandle::new(token_provider.clone(), 32, 32);
    tracing::info!("TwitchChatManagerActor created");

    let server_config_for_state = Arc::new(app_settings.server.clone());

    let lobby_manager_handle = LobbyManagerHandle::new(
        32,
        twitch_chat_manager_handle.clone(),
        app_settings.games.clone(),
        Arc::clone(&word_list_manager),
    );
    tracing::info!("LobbyManagerActor created");

    let app_state = AppState {
        lobby_manager: lobby_manager_handle,
        twitch_chat_manager: twitch_chat_manager_handle,
        word_list_manager,
        server_config: server_config_for_state,
        token_provider,
    };

    tracing::info!(
        server.port = app_settings.server.port,
        server.cors_origins.count = app_settings.server.cors_origins.len(),
        "Starting HTTP server"
    );

    run_server(app_state, app_settings.server).await?;

    tracing::info!("Application shutting down");
    Ok(())
}
