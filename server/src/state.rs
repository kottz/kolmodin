use std::sync::Arc;

use crate::config::ServerConfig;
use crate::content::GameContentCache;
use crate::lobby::LobbyManagerHandle;

#[derive(Clone)]
pub struct AppState {
    pub lobby_manager: LobbyManagerHandle,
    pub game_content_cache: Arc<GameContentCache>,
    pub server_config: Arc<ServerConfig>,
}
