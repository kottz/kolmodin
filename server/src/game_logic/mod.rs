// src/game_logic/mod.rs

use axum::extract::ws;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid; // For the Debug bound

mod game_two_echo;
mod hello_world_game;
// Re-export your game implementations
pub use game_two_echo::GameTwoEcho;
pub use hello_world_game::HelloWorldGame;

// Trait for game-specific logic using native async fn
// No async_trait crate needed now
pub trait GameLogic: Send + Sync + Debug {
    /// Called when a client connects to the game instance within the lobby.
    fn client_connected(
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) -> impl Future<Output = ()> + Send; // <--- ADD + Send HERE

    fn client_disconnected(&mut self, client_id: Uuid) -> impl Future<Output = ()> + Send; // <--- ADD + Send HERE

    fn handle_event(
        &mut self,
        client_id: Uuid,
        event_data: String,
    ) -> impl Future<Output = ()> + Send; // <--- ADD + Send HERE

    /// A method to check if the game instance is considered "empty" or "inactive".
    fn is_empty(&self) -> bool;

    /// Optional: A method to get the name or type of the game.
    fn game_type(&self) -> String {
        "UnknownGame".to_string()
    }
}
