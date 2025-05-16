// src/game_logic/mod.rs

use axum::extract::ws;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::twitch_integration::ParsedTwitchMessage;

// Add the new messages module and re-export its types
pub mod messages;
pub use messages::{ClientToServerMessage, ServerToClientMessage};

mod game_two_echo;
mod hello_world_game;

pub use game_two_echo::GameTwoEcho;
pub use hello_world_game::HelloWorldGame;

pub trait GameLogic: Send + Sync + Debug {
    fn client_connected(
        &mut self,
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    ) -> impl Future<Output = ()> + Send;

    fn client_disconnected(&mut self, client_id: Uuid) -> impl Future<Output = ()> + Send;

    // UPDATED: handle_event now takes a structured message
    fn handle_event(
        &mut self,
        client_id: Uuid,
        message: ClientToServerMessage, // Changed from String
    ) -> impl Future<Output = ()> + Send;

    fn handle_twitch_message(
        &mut self,
        message: ParsedTwitchMessage,
    ) -> impl Future<Output = ()> + Send;

    fn is_empty(&self) -> bool;

    fn game_type(&self) -> String {
        "UnknownGame".to_string()
    }

    // NEW: Helper to get a client's sender channel, used by LobbyActor for errors
    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>>;
}
