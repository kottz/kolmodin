// src/game_logic/mod.rs

use axum::extract::ws;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::twitch_integration::ParsedTwitchMessage;

// Add the new messages module and re-export its types
pub mod messages;
pub use messages::{ClientToServerMessage, ServerToClientMessage}; // These are now generic

// Game mode modules
pub mod deal_no_deal;
mod game_two_echo; // Example
mod hello_world_game; // Example // Your DND game

pub use deal_no_deal::DealNoDealGame;
pub use game_two_echo::GameTwoEcho;
pub use hello_world_game::HelloWorldGame;

pub trait GameLogic: Send + Sync + Debug {
    fn client_connected(
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) -> impl Future<Output = ()> + Send;

    fn client_disconnected(&mut self, client_id: Uuid) -> impl Future<Output = ()> + Send;

    // UPDATED: handle_event now takes the generic structured message
    fn handle_event(
        &mut self,
        client_id: Uuid,                // The ID of the client sending the command
        message: ClientToServerMessage, // The generic message wrapper
    ) -> impl Future<Output = ()> + Send;

    fn handle_twitch_message(
        &mut self,
        message: ParsedTwitchMessage,
    ) -> impl Future<Output = ()> + Send;

    fn is_empty(&self) -> bool;

    /// Returns the unique identifier for this game type (e.g., "DealNoDeal").
    /// This MUST match the `game_type_id` used in messages.
    fn game_type_id(&self) -> String;

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>>;

    /// Get all connected client IDs for broadcasting
    fn get_all_client_ids(&self) -> Vec<Uuid>;
}
