// src/game_logic/mod.rs

use axum::extract::ws;
use std::{fmt::Debug, future::Future, sync::Arc}; // Added Arc
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::twitch::ParsedTwitchMessage;

// Add the new messages module and re-export its types
pub mod messages;
pub use messages::{ClientToServerMessage, ServerToClientMessage}; // These are now generic

// Game mode modules
pub mod deal_no_deal;
pub mod med_andra_ord;

pub use deal_no_deal::DealNoDealGame;
pub use med_andra_ord::MedAndraOrdGameState;

#[derive(Debug, Clone, PartialEq)]
pub enum GameType {
    DealNoDeal,
    MedAndraOrd,
}

impl GameType {
    /// Get all available game types
    pub fn all() -> Vec<Self> {
        vec![GameType::DealNoDeal, GameType::MedAndraOrd]
    }

    /// Get all valid string identifiers for this game type
    pub fn aliases(&self) -> &'static [&'static str] {
        match self {
            GameType::DealNoDeal => &["dealnodeal", "dealornodeal"],
            GameType::MedAndraOrd => &["medandraord", "medandra", "ord"],
        }
    }

    /// Get the primary identifier for this game type
    pub fn primary_id(&self) -> &'static str {
        self.aliases()[0]
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let s_lower = s.to_lowercase();
        Self::all()
            .into_iter()
            .find(|game_type| game_type.aliases().iter().any(|alias| *alias == s_lower))
    }

    pub fn to_game_type_id(&self) -> String {
        match self {
            GameType::DealNoDeal => "DealNoDeal".to_string(),
            GameType::MedAndraOrd => "MedAndraOrd".to_string(),
        }
    }
}

// Factory functions for creating specific game instances
impl GameType {
    pub fn create_deal_no_deal_game() -> DealNoDealGame {
        DealNoDealGame::new()
    }

    // MedAndraOrdGameState creation is now handled by LobbyManagerActor,
    // which has access to the WordListManager to provide the necessary Arc<Vec<String>>.
    // If a generic factory is needed here, it would require a way to access words.
    // pub fn create_med_andra_ord_game(words: Arc<Vec<String>>) -> MedAndraOrdGameState {
    //     MedAndraOrdGameState::new(words)
    // }
}

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
