// src/game_logic/game_two_echo.rs

use super::{ClientToServerMessage, GameLogic, ServerToClientMessage}; // Updated imports
use crate::twitch_integration::ParsedTwitchMessage;
use axum::extract::ws;
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

#[derive(Debug)]
pub struct GameTwoEcho {
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
}

impl GameTwoEcho {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    async fn send_to_client(&self, client_id: &Uuid, message: ServerToClientMessage) {
        if let Some(tx) = self.clients.get(client_id) {
            match message.to_ws_text() {
                Ok(ws_msg) => {
                    if tx.send(ws_msg).await.is_err() {
                        tracing::warn!(
                            "GameTwoEcho: Failed to send message to client {}",
                            client_id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "GameTwoEcho: Failed to serialize message for client {}: {}",
                        client_id,
                        e
                    );
                }
            }
        }
    }

    async fn broadcast_to_all(&self, message: ServerToClientMessage) {
        match message.to_ws_text() {
            Ok(ws_msg) => {
                for (id, tx) in &self.clients {
                    if tx.send(ws_msg.clone()).await.is_err() {
                        tracing::warn!("GameTwoEcho: Failed to broadcast to client {}", id);
                    }
                }
            }
            Err(e) => {
                tracing::error!("GameTwoEcho: Failed to serialize broadcast message: {}", e);
            }
        }
    }
}

impl GameLogic for GameTwoEcho {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::info!("GameTwoEcho: Client {} connected.", client_id);
        self.clients.insert(client_id, client_tx);
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("GameTwoEcho: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    // UPDATED handle_event
    async fn handle_event(&mut self, client_id: Uuid, message: ClientToServerMessage) {
        tracing::debug!(
            "GameTwoEcho: Handling event from {}: {:?}",
            client_id,
            message
        );

        match message {
            ClientToServerMessage::Echo { message: payload } => {
                let response = ServerToClientMessage::EchoResponse {
                    original: payload.clone(),
                    processed: format!("Game 2 Echo of your original: {}", payload),
                };
                self.send_to_client(&client_id, response).await;
            }
            ClientToServerMessage::SendToSelf { message: payload } => {
                let response = ServerToClientMessage::PrivateMessage {
                    content: format!("Game 2 Private: {}", payload),
                };
                self.send_to_client(&client_id, response).await;
            }
            ClientToServerMessage::BroadcastAll { message: payload } => {
                let response = ServerToClientMessage::BroadcastMessage {
                    content: format!("Game 2 Broadcast: {}", payload),
                };
                self.broadcast_to_all(response).await;
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
        let response = ServerToClientMessage::TwitchMessageRelay {
            channel: message.channel.clone(),
            sender: message.sender_username.clone(),
            text: format!(
                "[G2 Twitch #{} by {}]: {}",
                message.channel, message.sender_username, message.text
            ),
        };
        self.broadcast_to_all(response).await;
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type(&self) -> String {
        "GameTwoEcho".to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }
}
