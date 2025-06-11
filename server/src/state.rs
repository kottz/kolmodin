use std::sync::Arc;

use crate::config::ServerConfig;
use crate::db::WordListManager;
use crate::lobby::LobbyManagerHandle;

#[derive(Clone)]
pub struct AppState {
    pub lobby_manager: LobbyManagerHandle,
    pub word_list_manager: Arc<WordListManager>,
    pub server_config: Arc<ServerConfig>,
}
