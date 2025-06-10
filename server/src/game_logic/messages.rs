use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Generic messages sent from any Game Client (WebSocket) to the Server.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "messageType", content = "payload")]
pub enum ClientToServerMessage {
    /// Sent by the client immediately after WebSocket connection to associate with a lobby.
    ConnectToLobby { lobby_id: Uuid },
    /// Sent by the client to explicitly leave the lobby and close the connection.
    /// This indicates the user intentionally wants to be removed from the lobby.
    LeaveLobby,
    /// For commands that are NOT specific to a game instance,
    /// e.g., authentication, lobby chat, or high-level controls.
    GlobalCommand {
        command_name: String, // e.g., "Echo", "JoinLobby"
        data: JsonValue,      // Payload for the global command
    },
    /// For commands directed at a specific, running game instance.
    GameSpecificCommand {
        /// Identifies the type of game this command is for (e.g., "DealNoDeal", "Chess").
        /// The Lobby/GameManager will use this to route the command.
        game_type_id: String,
        /// The actual game-specific command, serialized as JSON.
        /// The target game logic will deserialize this into its own command enum/struct.
        command_data: JsonValue,
    },
}

/// Generic messages sent from the Server to any Game Client.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "messageType", content = "payload")] // Using camelCase for JS convention
pub enum ServerToClientMessage {
    /// For events/responses that are NOT specific to a game instance.
    GlobalEvent {
        event_name: String, // e.g., "EchoResponse", "LobbyUpdate"
        data: JsonValue,    // Payload for the global event
    },
    /// For events/responses originating from a specific, running game instance.
    GameSpecificEvent {
        /// Identifies the type of game this event is from (e.g., "DealNoDeal", "Chess").
        /// The client will use this to know how to interpret the event_data.
        game_type_id: String,
        /// The actual game-specific event, serialized as JSON.
        /// The client game logic will deserialize this.
        event_data: JsonValue,
    },
    /// A general error message not tied to a specific game's internal logic error.
    /// Game-specific errors should be part of GameSpecificEvent.
    SystemError { message: String },
    /// For relaying Twitch chat messages to the game clients.
    TwitchMessageRelay {
        channel: String,
        sender: String,
        text: String,
    },
}

impl ServerToClientMessage {
    pub fn to_ws_text(&self) -> Result<axum::extract::ws::Message, serde_json::Error> {
        serde_json::to_string(self)
            .map(|json_string| axum::extract::ws::Message::Text(json_string.into()))
    }

    pub fn new_game_specific_event<S: Serialize>(
        game_type_id: String,
        game_specific_payload: &S,
    ) -> Result<Self, serde_json::Error> {
        let event_data = serde_json::to_value(game_specific_payload)?;
        Ok(ServerToClientMessage::GameSpecificEvent {
            game_type_id,
            event_data,
        })
    }

    pub fn new_global_event<S: Serialize>(
        event_name: String,
        global_payload: &S,
    ) -> Result<Self, serde_json::Error> {
        let data = serde_json::to_value(global_payload)?;
        Ok(ServerToClientMessage::GlobalEvent { event_name, data })
    }
}

pub fn client_message_from_ws_text(text: &str) -> Result<ClientToServerMessage, serde_json::Error> {
    serde_json::from_str(text)
}
