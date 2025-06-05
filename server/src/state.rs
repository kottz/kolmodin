use crate::db::WordListManager;
use crate::lobby::LobbyManagerHandle;
use crate::twitch::TwitchChatManagerActorHandle;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub lobby_manager: LobbyManagerHandle,
    pub twitch_chat_manager: TwitchChatManagerActorHandle,
    pub word_list_manager: Arc<WordListManager>,
}
