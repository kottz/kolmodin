use std::sync::Arc;

use dashmap::DashMap;
use uuid::Uuid;

use crate::config::{AppSettings, GamesConfig, ServerConfig};
use crate::content::GameContentCache;
use crate::lobby::{self, LobbyActorHandle, LobbyDetails};
use crate::twitch::TwitchServiceHandle;

#[derive(Clone)]
pub struct AppState {
    pub active_lobbies: Arc<DashMap<Uuid, LobbyActorHandle>>,
    pub game_content_cache: Arc<GameContentCache>,
    pub server_config: Arc<ServerConfig>,
    pub games_config: GamesConfig,
    pub twitch_service: TwitchServiceHandle,
    pub app_settings: Arc<AppSettings>,
}

impl AppState {
    pub async fn create_lobby(
        &self,
        requested_game_type: Option<String>,
        requested_twitch_channel: Option<String>,
    ) -> Result<LobbyDetails, String> {
        lobby::create_lobby(
            Arc::clone(&self.active_lobbies),
            self.games_config.clone(),
            Arc::clone(&self.game_content_cache),
            self.twitch_service.clone(),
            Arc::clone(&self.app_settings),
            requested_game_type,
            requested_twitch_channel,
        )
        .await
    }

    pub fn get_lobby_handle(&self, lobby_id: Uuid) -> Option<LobbyActorHandle> {
        self.active_lobbies
            .get(&lobby_id)
            .map(|entry| entry.value().clone())
    }
}
