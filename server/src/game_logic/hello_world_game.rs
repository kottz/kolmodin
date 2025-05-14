// src/game_logic/hello_world_game.rs

use super::GameLogic; // Use the trait from the parent module
use axum::extract::ws;
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

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
}

// No #[async_trait] needed
impl GameLogic for HelloWorldGame {
    async fn client_connected(
        // Now a regular async fn
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) {
        tracing::info!("HelloWorldGame: Client {} connected.", client_id);
        self.clients.insert(client_id, client_tx);
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("HelloWorldGame: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, client_id: Uuid, event_data: String) {
        tracing::debug!(
            "HelloWorldGame: Handling event from {}: {}",
            client_id,
            event_data
        );

        let mut parts = event_data.splitn(2, ' ');
        let command = parts.next().unwrap_or("").to_lowercase();
        let payload = parts.next().unwrap_or("Hello World (default from game)");

        let message_to_send = payload.to_string();

        match command.as_str() {
            "send_to_self" => {
                if let Some(sender_tx) = self.clients.get(&client_id) {
                    if sender_tx
                        .send(ws::Message::Text(
                            format!("Game1 Private: {}", message_to_send).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("HelloWorldGame: Failed to send to self {}", client_id);
                    }
                }
            }
            "broadcast_all" => {
                for (target_id, tx) in &self.clients {
                    if tx
                        .send(ws::Message::Text(
                            format!("Game1 Broadcast: {}", message_to_send).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("HelloWorldGame: Failed to broadcast to {}", target_id);
                    }
                }
            }
            "broadcast_except_self" => {
                for (target_id, tx) in &self.clients {
                    if *target_id != client_id {
                        if tx
                            .send(ws::Message::Text(
                                format!("Game1 Broadcast (others): {}", message_to_send).into(),
                            ))
                            .await
                            .is_err()
                        {
                            tracing::warn!(
                                "HelloWorldGame: Failed to broadcast (others) to {}",
                                target_id
                            );
                        }
                    }
                }
            }
            _ => {
                if let Some(sender_tx) = self.clients.get(&client_id) {
                    if sender_tx
                        .send(ws::Message::Text(
                            format!("Game1 Echo: {}", event_data).into(),
                        ))
                        .await
                        .is_err()
                    {
                        tracing::warn!("HelloWorldGame: Failed to send echo to {}", client_id);
                    }
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type(&self) -> String {
        "HelloWorldGame".to_string()
    }
}
