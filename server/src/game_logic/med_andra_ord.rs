use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use uuid::Uuid;

use crate::game_logic::GameLogic;
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::twitch_integration::ParsedTwitchMessage;

const GAME_TYPE_ID_MED_ANDRA_ORD: &str = "MedAndraOrd";
const ROUND_DURATION_SECONDS: u32 = 60;

// Static list of Swedish words that are fun to describe
const SWEDISH_WORDS: &[&str] = &[
    "jordgubbe",      // strawberry
    "paraply",        // umbrella
    "kylskåp",        // refrigerator
    "tandborste",     // toothbrush
    "regnbåge",       // rainbow
    "eldgaffel",      // fork
    "telefon",        // telephone
    "cykel",          // bicycle
    "sommarsemester", // summer vacation
    "chokladkaka",    // chocolate bar
];

// Admin Commands (Client -> Server)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
    PassWord,
    ResetGame,
    SetTargetPoints { points: u32 },
}

// Game Events (Server -> Client)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    FullStateUpdate(MedAndraOrdGameState),
    WordChanged { word: String },
    PlayerScored { player: String, points: u32 },
    GamePhaseChanged { new_phase: GamePhase },
    TimerUpdate { seconds_remaining: u32 },
}

// Game Phases
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum GamePhase {
    Setup,
    Playing { current_word: String },
    GameOver { winner: String },
}

// Main Game State
#[derive(Serialize, Deserialize, Debug)]
pub struct MedAndraOrdGameState {
    #[serde(skip)]
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,

    pub phase: GamePhase,
    pub target_points: u32,
    pub player_scores: HashMap<String, u32>,
    pub timer_seconds_remaining: u32,

    #[serde(skip)]
    words: Vec<String>,
    #[serde(skip)]
    used_words: HashSet<String>,
    #[serde(skip)]
    timer_handle: Option<JoinHandle<()>>,
}

impl Clone for MedAndraOrdGameState {
    fn clone(&self) -> Self {
        Self {
            clients: HashMap::new(), // Don't clone clients
            phase: self.phase.clone(),
            target_points: self.target_points,
            player_scores: self.player_scores.clone(),
            timer_seconds_remaining: self.timer_seconds_remaining,
            words: self.words.clone(),
            used_words: self.used_words.clone(),
            timer_handle: None, // Don't clone timer handle
        }
    }
}

impl MedAndraOrdGameState {
    pub fn new() -> Self {
        let words: Vec<String> = SWEDISH_WORDS.iter().map(|&s| s.to_string()).collect();

        Self {
            clients: HashMap::new(),
            phase: GamePhase::Setup,
            target_points: 10,
            player_scores: HashMap::new(),
            timer_seconds_remaining: ROUND_DURATION_SECONDS,
            words,
            used_words: HashSet::new(),
            timer_handle: None,
        }
    }

    // Helper to send events to specific client
    async fn send_game_event_to_client(&self, client_id: &Uuid, event_payload: GameEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_MED_ANDRA_ORD.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.send_generic_message_to_client(client_id, wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    "MedAndraOrd: Failed to serialize GameEvent for client {}: {}",
                    client_id,
                    e
                );
            }
        }
    }

    // Helper to broadcast events to all clients
    async fn broadcast_game_event_to_all(&self, event_payload: GameEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_MED_ANDRA_ORD.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.broadcast_generic_message_to_all(wrapped_message).await;
            }
            Err(e) => {
                tracing::error!(
                    "MedAndraOrd: Failed to serialize GameEvent for broadcast: {}",
                    e
                );
            }
        }
    }

    // Send full state update
    async fn broadcast_full_state_update(&mut self) {
        let state_clone = self.clone();
        self.broadcast_game_event_to_all(GameEvent::FullStateUpdate(state_clone))
            .await;
    }

    // Generic message sending helpers
    async fn send_generic_message_to_client(
        &self,
        client_id: &Uuid,
        message: GenericServerToClientMessage,
    ) {
        if let Some(tx) = self.clients.get(client_id) {
            if let Ok(ws_msg) = message.to_ws_text() {
                if tx.send(ws_msg).await.is_err() {
                    tracing::warn!("MedAndraOrd: Failed to send to client {}", client_id);
                }
            }
        }
    }

    async fn broadcast_generic_message_to_all(&self, message: GenericServerToClientMessage) {
        if self.clients.is_empty() {
            return;
        }
        if let Ok(ws_msg) = message.to_ws_text() {
            for (id, tx) in &self.clients {
                if tx.send(ws_msg.clone()).await.is_err() {
                    tracing::warn!("MedAndraOrd: Failed to broadcast to client {}", id);
                }
            }
        }
    }

    // Game-specific methods
    async fn handle_start_game(&mut self) {
        if self.phase != GamePhase::Setup {
            return;
        }

        self.used_words.clear();
        self.player_scores.clear();
        self.timer_seconds_remaining = ROUND_DURATION_SECONDS;

        if let Some(word) = self.get_next_word() {
            self.phase = GamePhase::Playing {
                current_word: word.clone(),
            };
            self.start_timer().await;
            self.broadcast_game_event_to_all(GameEvent::WordChanged { word })
                .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
        }
    }

    async fn handle_pass_word(&mut self) {
        if let GamePhase::Playing { .. } = &self.phase {
            if let Some(word) = self.get_next_word() {
                self.phase = GamePhase::Playing {
                    current_word: word.clone(),
                };
                self.broadcast_game_event_to_all(GameEvent::WordChanged { word })
                    .await;
                self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                    new_phase: self.phase.clone(),
                })
                .await;
            }
        }
    }

    async fn handle_reset_game(&mut self) {
        self.stop_timer().await;
        self.phase = GamePhase::Setup;
        self.player_scores.clear();
        self.used_words.clear();
        self.timer_seconds_remaining = ROUND_DURATION_SECONDS;
        self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
            new_phase: self.phase.clone(),
        })
        .await;
    }

    fn handle_set_target_points(&mut self, points: u32) {
        if self.phase == GamePhase::Setup {
            self.target_points = points;
        }
    }

    fn get_next_word(&mut self) -> Option<String> {
        let available_words: Vec<String> = self
            .words
            .iter()
            .filter(|word| !self.used_words.contains(*word))
            .cloned()
            .collect();

        if available_words.is_empty() {
            // Reset used words if all have been used
            self.used_words.clear();
            self.words.choose(&mut thread_rng()).cloned()
        } else {
            available_words.choose(&mut thread_rng()).cloned()
        }
    }

    async fn process_correct_guess(&mut self, player: &str) {
        let current_score = self.player_scores.get(player).unwrap_or(&0) + 1;
        self.player_scores.insert(player.to_string(), current_score);

        self.broadcast_game_event_to_all(GameEvent::PlayerScored {
            player: player.to_string(),
            points: current_score,
        })
        .await;

        // Check if player won
        if current_score >= self.target_points {
            self.stop_timer().await;
            self.phase = GamePhase::GameOver {
                winner: player.to_string(),
            };
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            return;
        }

        // Get next word
        if let Some(word) = self.get_next_word() {
            self.phase = GamePhase::Playing {
                current_word: word.clone(),
            };
            self.broadcast_game_event_to_all(GameEvent::WordChanged { word })
                .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
        }
    }

    async fn start_timer(&mut self) {
        self.stop_timer().await;
        self.timer_seconds_remaining = ROUND_DURATION_SECONDS;

        // Note: In a real implementation, you'd need to handle the timer properly
        // This is a simplified version - you might need to use Arc<Mutex<>> for shared state
        // and send updates through a channel
    }

    async fn stop_timer(&mut self) {
        if let Some(handle) = self.timer_handle.take() {
            handle.abort();
        }
    }
}

impl GameLogic for MedAndraOrdGameState {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::info!("MedAndraOrd: Client {} connected.", client_id);
        self.clients.insert(client_id.clone(), client_tx);

        // Send initial state to new client
        let state_clone = self.clone();
        self.send_game_event_to_client(&client_id, GameEvent::FullStateUpdate(state_clone))
            .await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("MedAndraOrd: Client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, client_id: Uuid, message: GenericClientToServerMessage) {
        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
            } => {
                if game_type_id != self.game_type_id() {
                    tracing::warn!("MedAndraOrd: Wrong game_type_id: {}", game_type_id);
                    return;
                }

                match serde_json::from_value::<AdminCommand>(command_data) {
                    Ok(cmd) => {
                        match cmd {
                            AdminCommand::StartGame => self.handle_start_game().await,
                            AdminCommand::PassWord => self.handle_pass_word().await,
                            AdminCommand::ResetGame => self.handle_reset_game().await,
                            AdminCommand::SetTargetPoints { points } => {
                                self.handle_set_target_points(points)
                            }
                        }
                        self.broadcast_full_state_update().await;
                    }
                    Err(e) => {
                        tracing::error!("MedAndraOrd: Failed to deserialize command: {}", e);
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { command_name, data } => {
                tracing::trace!("MedAndraOrd: Received GlobalCommand: {}", command_name);
            }
            _ => {
                tracing::warn!("MedAndraOrd: Unrecognized message type");
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        tracing::info!(
            "MedAndraOrd: Twitch message from {}: {}",
            message.sender_username,
            message.text
        );

        // Process Twitch chat messages based on game phase
        if let GamePhase::Playing { current_word } = &self.phase {
            let guess = message.text.trim().to_lowercase();
            let target_word = current_word.to_lowercase();

            if guess == target_word {
                tracing::info!(
                    "MedAndraOrd: Correct guess '{}' from {}",
                    guess,
                    message.sender_username
                );
                self.used_words.insert(current_word.clone());
                self.process_correct_guess(&message.sender_username).await;
                self.broadcast_full_state_update().await;
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_MED_ANDRA_ORD.to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }

    fn get_all_client_ids(&self) -> Vec<Uuid> {
        self.clients.keys().copied().collect()
    }
}
