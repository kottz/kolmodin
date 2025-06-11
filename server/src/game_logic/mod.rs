use axum::extract::ws;
use std::{fmt::Debug, future::Future};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::twitch::ParsedTwitchMessage;

#[derive(Debug, Clone, PartialEq)]
pub enum EventHandlingResult {
    /// Event was handled normally, no special actions needed
    Handled,
    /// Client should be disconnected (e.g., due to LeaveLobby request)
    DisconnectClient,
}

pub mod messages;
pub use messages::{ClientToServerMessage, ServerToClientMessage};

pub mod utils;

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
    pub fn all() -> Vec<Self> {
        vec![GameType::DealNoDeal, GameType::MedAndraOrd]
    }

    pub fn aliases(&self) -> &'static [&'static str] {
        match self {
            GameType::DealNoDeal => &["dealnodeal", "dealornodeal"],
            GameType::MedAndraOrd => &["medandraord", "medandra", "ord"],
        }
    }

    pub fn primary_id(&self) -> &'static str {
        self.aliases()[0]
    }
}

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
        message: ClientToServerMessage,
    ) -> impl Future<Output = EventHandlingResult> + Send;

    fn handle_twitch_message(
        &mut self,
        message: ParsedTwitchMessage,
    ) -> impl Future<Output = ()> + Send;

    fn is_empty(&self) -> bool;

    fn game_type_id(&self) -> String;

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>>;

    fn get_all_client_ids(&self) -> Vec<Uuid>;
}
