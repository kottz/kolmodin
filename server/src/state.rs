use std::sync::Arc;

use crate::config::ServerConfig;
use crate::db::WordListManager;
use crate::lobby::LobbyManagerHandle;
use crate::twitch::actors::TwitchChatManagerActorHandle;
use crate::twitch::token_provider::TokenProvider;

#[derive(Clone)]
pub struct AppState {
    pub lobby_manager: LobbyManagerHandle,
    pub twitch_chat_manager: TwitchChatManagerActorHandle,
    pub word_list_manager: Arc<WordListManager>,
    pub server_config: Arc<ServerConfig>,
    pub token_provider: TokenProvider,
}
