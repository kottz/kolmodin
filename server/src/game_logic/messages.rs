// src/game_logic/messages.rs
use serde::{Deserialize, Serialize};
// Use Uuid if you plan to include it directly in message payloads, otherwise not strictly needed here.
// use uuid::Uuid;

/// Messages sent from the Game Client (WebSocket) to the Server (GameLogic).
/// These typically represent actions the player wants to take.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command", content = "payload")] // Using "command" as the tag for C2S messages
pub enum ClientToServerMessage {
    /// A generic message to be echoed back, possibly processed.
    Echo { message: String },
    /// A message intended only for the sender.
    SendToSelf { message: String },
    /// A message to be broadcast to all clients in the game/lobby.
    BroadcastAll { message: String },
    // Example of a more game-specific command:
    // PlayCard { card_id: String, target_player_id: Option<Uuid> },
}

/// Messages sent from the Server (GameLogic) to the Game Client (WebSocket).
/// These typically represent game state updates, responses, or errors.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event", content = "data")] // Using "event" as the tag for S2C messages
pub enum ServerToClientMessage {
    /// Response to an Echo command.
    EchoResponse { original: String, processed: String },
    /// A private message directed to this client.
    PrivateMessage { content: String },
    /// A message broadcast to all clients.
    BroadcastMessage { content: String },
    /// For relaying Twitch chat messages to the game clients.
    TwitchMessageRelay {
        channel: String,
        sender: String,
        text: String,
    },
    /// For game-specific state updates that don't fit other categories.
    /// `serde_json::Value` allows for flexible, arbitrary JSON data.
    GameUpdate { update_data: serde_json::Value },
    /// To inform the client of an error.
    Error { message: String },
    // Example of a more game-specific event:
    // CardPlayed { player_id: Uuid, card_id: String, outcome: String },
}

impl ServerToClientMessage {
    /// Helper to convert this enum into a WebSocket text message.
    pub fn to_ws_text(&self) -> Result<axum::extract::ws::Message, serde_json::Error> {
        serde_json::to_string(self)
            .map(|json_string| axum::extract::ws::Message::Text(json_string.into()))
        //                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        //                               This is the changed line.
        // We explicitly create a closure that takes the `String` (json_string)
        // and passes it to the `axum::extract::ws::Message::Text` variant.
    }
}

/// Helper to convert a string (presumably from a WebSocket text message) into ClientToServerMessage.
pub fn client_message_from_ws_text(text: &str) -> Result<ClientToServerMessage, serde_json::Error> {
    serde_json::from_str(text)
}
