// src/game_logic/mod.rs

use axum::extract::ws;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

// We'll need ParsedTwitchMessage here for the trait method
// This assumes twitch_integration is a sibling module or accessible via crate root
use crate::twitch_integration::ParsedTwitchMessage;

mod game_two_echo;
mod hello_world_game;
// Re-export your game implementations
pub use game_two_echo::GameTwoEcho;
pub use hello_world_game::HelloWorldGame;

pub trait GameLogic: Send + Sync + Debug {
    fn client_connected(
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) -> impl Future<Output = ()> + Send;

    fn client_disconnected(&mut self, client_id: Uuid) -> impl Future<Output = ()> + Send;

    fn handle_event(
        &mut self,
        client_id: Uuid,
        event_data: String,
    ) -> impl Future<Output = ()> + Send;

    /// NEW: Called when a Twitch message is received for a channel this lobby is subscribed to.
    fn handle_twitch_message(
        &mut self,
        message: ParsedTwitchMessage,
    ) -> impl Future<Output = ()> + Send;

    fn is_empty(&self) -> bool;

    fn game_type(&self) -> String {
        "UnknownGame".to_string()
    }
}
