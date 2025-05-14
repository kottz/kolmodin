// src/game_logic/game_two_echo.rs

use super::GameLogic; // Use the trait from the parent module
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
}

// No #[async_trait] needed
impl GameLogic for GameTwoEcho {
    async fn client_connected(
        // Now a regular async fn
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) {
        tracing::info!("GameTwoEcho: Client {} connected.", client_id);
        self.clients.insert(client_id, client_tx);
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("GameTwoEcho: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, client_id: Uuid, event_data: String) {
        tracing::debug!(
            "GameTwoEcho: Handling event from {}: {}",
            client_id,
            event_data
        );

        let mut parts = event_data.splitn(2, ' ');
        let command = parts.next().unwrap_or("").to_lowercase();
        let payload = parts.next().unwrap_or("Game 2 says Hello (default)");

        let message_to_send = format!("Game 2: {}", payload);

        match command.as_str() {
            "send_to_self" => {
                if let Some(sender_tx) = self.clients.get(&client_id) {
                    if sender_tx
                        .send(ws::Message::Text(
                            format!("Private: {}", message_to_send).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("GameTwoEcho: Failed to send to self {}", client_id);
                    }
                }
            }
            "broadcast_all" => {
                for (target_id, tx) in &self.clients {
                    if tx
                        .send(ws::Message::Text(
                            format!("Broadcast: {}", message_to_send).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("GameTwoEcho: Failed to broadcast to {}", target_id);
                    }
                }
            }
            "broadcast_except_self" => {
                for (target_id, tx) in &self.clients {
                    if *target_id != client_id
                        && tx
                            .send(ws::Message::Text(
                                format!("Broadcast (others): {}", message_to_send).into(),
                            ))
                            .await
                            .is_err()
                    {
                        tracing::warn!(
                            "GameTwoEcho: Failed to broadcast (others) to {}",
                            target_id
                        );
                    }
                }
            }
            _ => {
                if let Some(sender_tx) = self.clients.get(&client_id) {
                    if sender_tx
                        .send(ws::Message::Text(
                            format!("Game 2 Echo of your original: {}", event_data).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("GameTwoEcho: Failed to send echo to {}", client_id);
                    }
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type(&self) -> String {
        "GameTwoEcho".to_string()
    }
}
