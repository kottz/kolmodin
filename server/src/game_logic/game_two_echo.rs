// src/game_logic/game_two_echo/mod.rs
use axum::extract::ws;
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

// Generic message types from the main messages module
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};

use crate::game_logic::GameLogic;
use crate::twitch_integration::ParsedTwitchMessage;

const GAME_TYPE_ID_GAME_TWO_ECHO: &str = "GameTwoEcho";
// src/game_logic/game_two_echo/types.rs
use serde::{Deserialize, Serialize};

// --- GameTwoEcho Specific Commands (Client -> Server) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")] // Discriminator for GameTwoEcho commands
pub enum GameTwoCommand {
    Echo { message: String },
    SendToSelf { message: String },
    BroadcastAll { message: String },
    // You could add more commands specific to GameTwoEcho here
    // Example: SetEchoPrefix { prefix: String }
}

// --- GameTwoEcho Specific Events (Server -> Client) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")] // Discriminator for GameTwoEcho events
pub enum GameTwoEvent {
    EchoResponse { original: String, processed: String },
    PrivateMessage { content: String },
    BroadcastMessage { content: String },
    // Example: EchoPrefixChanged { new_prefix: String }
    // Example: GameState { current_prefix: String }
}
#[derive(Debug)]
pub struct GameTwoEcho {
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
    // Example of game-specific state:
    // echo_prefix: String,
}

impl GameTwoEcho {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            // echo_prefix: "Game 2 Echo: ".to_string(), // Initialize game-specific state
        }
    }

    // Helper to send a GameTwoEvent to a specific client
    async fn send_game_two_event_to_client(&self, client_id: &Uuid, event_payload: GameTwoEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_GAME_TWO_ECHO.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.send_generic_message_to_client(client_id, wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    "GameTwoEcho: Failed to serialize GameTwoEvent for client {}: {}",
                    client_id,
                    e
                );
            }
        }
    }

    // Helper to broadcast a GameTwoEvent to all connected clients
    async fn broadcast_game_two_event_to_all(&self, event_payload: GameTwoEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_GAME_TWO_ECHO.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.broadcast_generic_message_to_all(wrapped_message).await;
            }
            Err(e) => {
                tracing::error!(
                    "GameTwoEcho: Failed to serialize GameTwoEvent for broadcast: {}",
                    e
                );
            }
        }
    }

    // Underlying generic sender methods
    async fn send_generic_message_to_client(
        &self,
        client_id: &Uuid,
        message: GenericServerToClientMessage,
    ) {
        if let Some(tx) = self.clients.get(client_id) {
            match message.to_ws_text() {
                Ok(ws_msg) => {
                    if tx.send(ws_msg).await.is_err() {
                        tracing::warn!(
                            "GameTwoEcho: Failed to send generic message to client {}",
                            client_id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "GameTwoEcho: Failed to serialize generic message for client {}: {}",
                        client_id,
                        e
                    );
                }
            }
        }
    }

    async fn broadcast_generic_message_to_all(&self, message: GenericServerToClientMessage) {
        if self.clients.is_empty() {
            return;
        }
        match message.to_ws_text() {
            Ok(ws_msg) => {
                for (id, tx) in &self.clients {
                    if tx.send(ws_msg.clone()).await.is_err() {
                        tracing::warn!(
                            "GameTwoEcho: Failed to broadcast generic message to client {}",
                            id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "GameTwoEcho: Failed to serialize generic message for broadcast: {}",
                    e
                );
            }
        }
    }
}

impl GameLogic for GameTwoEcho {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::info!("GameTwoEcho: Client {} connected.", client_id);
        self.clients.insert(client_id.clone(), client_tx);

        let welcome_event = GameTwoEvent::PrivateMessage {
            content: format!("Welcome to GameTwoEcho, Client {}!", client_id),
        };
        self.send_game_two_event_to_client(&client_id, welcome_event)
            .await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("GameTwoEcho: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, client_id: Uuid, message: GenericClientToServerMessage) {
        tracing::debug!(
            "GameTwoEcho: Handling event from {}: {:?}",
            client_id,
            message
        );

        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
                // game_instance_id,
            } => {
                if game_type_id != self.game_type_id() {
                    tracing::warn!(
                        "GameTwoEcho: Received command for wrong game type: {}. Expected: {}",
                        game_type_id,
                        self.game_type_id()
                    );
                    let err_msg = GenericServerToClientMessage::SystemError {
                        message: format!(
                            "Command intended for game type '{}', but this is '{}'.",
                            game_type_id,
                            self.game_type_id()
                        ),
                    };
                    self.send_generic_message_to_client(&client_id, err_msg)
                        .await;
                    return;
                }

                match serde_json::from_value::<GameTwoCommand>(command_data.clone()) {
                    Ok(g2_command) => {
                        match g2_command {
                            GameTwoCommand::Echo { message: payload } => {
                                let response_payload = GameTwoEvent::EchoResponse {
                                    original: payload.clone(),
                                    // You can use self.echo_prefix here if you add it to the struct
                                    processed: format!("Game 2 Echo of your original: {}", payload),
                                };
                                self.send_game_two_event_to_client(&client_id, response_payload)
                                    .await;
                            }
                            GameTwoCommand::SendToSelf { message: payload } => {
                                let response_payload = GameTwoEvent::PrivateMessage {
                                    content: format!("Game 2 Private: {}", payload),
                                };
                                self.send_game_two_event_to_client(&client_id, response_payload)
                                    .await;
                            }
                            GameTwoCommand::BroadcastAll { message: payload } => {
                                let response_payload = GameTwoEvent::BroadcastMessage {
                                    content: format!("Game 2 Broadcast: {}", payload),
                                };
                                self.broadcast_game_two_event_to_all(response_payload).await;
                            } // Handle other GameTwoCommands like SetEchoPrefix here
                              // GameTwoCommand::SetEchoPrefix { prefix } => {
                              //     self.echo_prefix = prefix.clone();
                              //     let event = GameTwoEvent::EchoPrefixChanged { new_prefix: prefix };
                              //     self.broadcast_game_two_event_to_all(event).await;
                              // }
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "GameTwoEcho: Failed to deserialize GameTwoCommand from client {}: {}. Payload: {:?}",
                            client_id,
                            e,
                            command_data
                        );
                        let err_msg = GenericServerToClientMessage::SystemError {
                            message: format!("Invalid GameTwoEcho command format: {}", e),
                        };
                        self.send_generic_message_to_client(&client_id, err_msg)
                            .await;
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { command_name, data } => {
                tracing::debug!(
                    "GameTwoEcho: Received GlobalCommand (unhandled by GameTwoEcho): name {}, data {:?}",
                    command_name,
                    data
                );
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        tracing::info!(
            "GameTwoEcho: Received Twitch message in channel #{}: <{}> {}",
            message.channel,
            message.sender_username,
            message.text
        );

        // TwitchMessageRelay is a global event type
        let response = GenericServerToClientMessage::TwitchMessageRelay {
            channel: message.channel.clone(),
            sender: message.sender_username.clone(),
            text: format!(
                // GameTwoEcho can customize the text if it wants before relaying
                "[G2 Twitch Relay in #{}] <{}>: {}",
                message.channel, message.sender_username, message.text
            ),
        };
        self.broadcast_generic_message_to_all(response).await;
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_GAME_TWO_ECHO.to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }
}
