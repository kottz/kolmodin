use crate::lobby::LobbyManagerHandle;
use crate::twitch::TwitchChatManagerActorHandle;

#[derive(Clone)]
pub struct AppState {
    pub lobby_manager: LobbyManagerHandle,
    pub twitch_chat_manager: TwitchChatManagerActorHandle,
}
