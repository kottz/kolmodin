// src/main.rs

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// --- Module Declarations ---
mod config;
mod error;
mod game_logic;
mod lobby;
mod state;
mod twitch;
mod web;

// --- Imports ---
use crate::config::load_settings;
use crate::error::Result as AppResult;
use crate::lobby::LobbyManagerHandle;
use crate::state::AppState;
use crate::twitch::TwitchChatManagerActorHandle;
use crate::twitch::fetch_twitch_app_access_token;
use crate::web::run_server;

#[tokio::main]
async fn main() -> AppResult<()> {
    // Setup tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=info,tower_http=debug,{}=trace",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load .env file if present (optional)
    // dotenvy::dotenv().ok();

    // Load Configuration
    let app_settings = load_settings()?;

    // Fetch Twitch App Access Token
    let app_oauth_token = Arc::new(
        fetch_twitch_app_access_token(
            &app_settings.twitch.client_id,
            &app_settings.twitch.client_secret,
        )
        .await?,
    );
    tracing::info!("Successfully fetched Twitch App Access Token.");

    // Initialize Twitch Chat Manager and Lobby Manager
    let twitch_chat_manager_handle = TwitchChatManagerActorHandle::new(app_oauth_token, 32, 32);
    let lobby_manager_handle =
        LobbyManagerHandle::new(32, twitch_chat_manager_handle.clone(), app_settings.games);

    // Create AppState
    let app_state = AppState {
        lobby_manager: lobby_manager_handle,
        twitch_chat_manager: twitch_chat_manager_handle,
    };

    // Run the web server
    run_server(app_state, app_settings.server).await?;

    Ok(())
}
