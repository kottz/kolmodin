// src/game_logic/hello_world_game/mod.rs
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
use serde::{Deserialize, Serialize};

const GAME_TYPE_ID_HELLO_WORLD: &str = "HelloWorldGame";

// --- HelloWorldGame Specific Commands (Client -> Server) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")] // Discriminator for HelloWorldGame commands
pub enum HelloWorldCommand {
    Echo { message: String },
    SendToSelf { message: String },
    BroadcastAll { message: String },
}

// --- HelloWorldGame Specific Events (Server -> Client) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")] // Discriminator for HelloWorldGame events
pub enum HelloWorldEvent {
    EchoResponse { original: String, processed: String },
    PrivateMessage { content: String },
    BroadcastMessage { content: String },
    // If HelloWorldGame had its own specific state to send:
    // GameState { current_message: Option<String> }
}
#[derive(Debug)]
pub struct HelloWorldGame {
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
}

impl HelloWorldGame {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    // Helper to send a HelloWorldEvent to a specific client
    async fn send_hello_world_event_to_client(
        &self,
        client_id: &Uuid,
        event_payload: HelloWorldEvent,
    ) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_HELLO_WORLD.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.send_generic_message_to_client(client_id, wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    "HelloWorldGame: Failed to serialize HelloWorldEvent for client {}: {}",
                    client_id,
                    e
                );
            }
        }
    }

    // Helper to broadcast a HelloWorldEvent to all connected clients
    async fn broadcast_hello_world_event_to_all(&self, event_payload: HelloWorldEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_HELLO_WORLD.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.broadcast_generic_message_to_all(wrapped_message).await;
            }
            Err(e) => {
                tracing::error!(
                    "HelloWorldGame: Failed to serialize HelloWorldEvent for broadcast: {}",
                    e
                );
            }
        }
    }

    // Underlying generic sender methods (could be part of a base struct or utility in a larger system)
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
                            "HelloWorldGame: Failed to send generic message to client {}",
                            client_id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "HelloWorldGame: Failed to serialize generic message for client {}: {}",
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
                            "HelloWorldGame: Failed to broadcast generic message to client {}",
                            id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "HelloWorldGame: Failed to serialize generic message for broadcast: {}",
                    e
                );
            }
        }
    }
}

impl GameLogic for HelloWorldGame {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::info!("HelloWorldGame: Client {} connected.", client_id);
        self.clients.insert(client_id.clone(), client_tx);
        // Optionally send a welcome message or initial state specific to HelloWorldGame
        let welcome_event = HelloWorldEvent::PrivateMessage {
            content: format!("Welcome to HelloWorldGame, Client {}!", client_id),
        };
        self.send_hello_world_event_to_client(&client_id, welcome_event)
            .await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("HelloWorldGame: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, client_id: Uuid, message: GenericClientToServerMessage) {
        tracing::debug!(
            "HelloWorldGame: Handling event from {}: {:?}",
            client_id,
            message
        );

        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
                // game_instance_id, // if you use it
            } => {
                if game_type_id != self.game_type_id() {
                    tracing::warn!(
                        "HelloWorldGame: Received command for wrong game type: {}. Expected: {}",
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

                // Attempt to deserialize command_data into HelloWorldCommand
                match serde_json::from_value::<HelloWorldCommand>(command_data.clone()) {
                    Ok(hw_command) => match hw_command {
                        HelloWorldCommand::Echo { message: payload } => {
                            let response_payload = HelloWorldEvent::EchoResponse {
                                original: payload.clone(),
                                processed: format!("HelloWorldGame Echo: {}", payload),
                            };
                            self.send_hello_world_event_to_client(&client_id, response_payload)
                                .await;
                        }
                        HelloWorldCommand::SendToSelf { message: payload } => {
                            let response_payload = HelloWorldEvent::PrivateMessage {
                                content: format!("HelloWorldGame Private: {}", payload),
                            };
                            self.send_hello_world_event_to_client(&client_id, response_payload)
                                .await;
                        }
                        HelloWorldCommand::BroadcastAll { message: payload } => {
                            let response_payload = HelloWorldEvent::BroadcastMessage {
                                content: format!("HelloWorldGame Broadcast: {}", payload),
                            };
                            self.broadcast_hello_world_event_to_all(response_payload)
                                .await;
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "HelloWorldGame: Failed to deserialize HelloWorldCommand from client {}: {}. Payload: {:?}",
                            client_id,
                            e,
                            command_data
                        );
                        let err_msg = GenericServerToClientMessage::SystemError {
                            message: format!("Invalid HelloWorldGame command format: {}", e),
                        };
                        self.send_generic_message_to_client(&client_id, err_msg)
                            .await;
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { command_name, data } => {
                tracing::debug!(
                    "HelloWorldGame: Received GlobalCommand (unhandled by HelloWorldGame): name {}, data {:?}",
                    command_name,
                    data
                );
                // Example: If HelloWorldGame wanted to respond to a global "Echo"
                if command_name == "Echo" {
                    if let Ok(payload_str) = serde_json::from_value::<String>(data) {
                        let response = GenericServerToClientMessage::new_global_event(
                            "EchoResponse".to_string(),
                            &format!("HelloWorldGame saw Global Echo: {}", payload_str),
                        )
                        .unwrap(); // Handle error in production
                        self.send_generic_message_to_client(&client_id, response)
                            .await;
                    }
                }
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        tracing::info!(
            "HelloWorldGame: Received Twitch message in channel #{}: <{}> {}",
            message.channel,
            message.sender_username,
            message.text
        );

        // TwitchMessageRelay is a global event type, so we can construct it directly.
        let response = GenericServerToClientMessage::TwitchMessageRelay {
            channel: message.channel.clone(),
            sender: message.sender_username.clone(),
            text: message.text.clone(),
        };
        // Use the generic broadcast method
        self.broadcast_generic_message_to_all(response).await;
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_HELLO_WORLD.to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }
}
