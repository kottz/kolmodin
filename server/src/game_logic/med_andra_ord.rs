use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::game_logic::utils::is_guess_acceptable;
use crate::game_logic::{EventHandlingResult, GameLogic};
use crate::twitch::ParsedTwitchMessage;

const GAME_TYPE_ID_MED_ANDRA_ORD: &str = "MedAndraOrd";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
    PassWord,
    ResetGame,
    SetTargetPoints { points: u32 },
    SetGameDuration { seconds: u32 },
    SetPointLimitEnabled { enabled: bool },
    SetTimeLimitEnabled { enabled: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    FullStateUpdate(MedAndraOrdGameState),
    WordChanged { word: String, is_placeholder: bool },
    PlayerScored { player: String, points: u32 },
    GamePhaseChanged { new_phase: GamePhase },
    GameTimeUpdate { remaining_seconds: u64 },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum GamePhase {
    Setup,
    Playing { current_word: String },
    GameOver { winner: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MedAndraOrdGameState {
    #[serde(skip)]
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,

    pub phase: GamePhase,
    pub target_points: u32,
    pub game_duration_seconds: u64,
    pub point_limit_enabled: bool,
    pub time_limit_enabled: bool,
    pub player_scores: HashMap<String, u32>,

    #[serde(skip)]
    current_word_list: Arc<Vec<String>>,
    #[serde(skip)]
    local_used_words: HashSet<String>,
    #[serde(skip)]
    game_start_time: Option<Instant>,
}

impl Clone for MedAndraOrdGameState {
    fn clone(&self) -> Self {
        Self {
            clients: HashMap::new(),
            phase: self.phase.clone(),
            target_points: self.target_points,
            game_duration_seconds: self.game_duration_seconds,
            point_limit_enabled: self.point_limit_enabled,
            time_limit_enabled: self.time_limit_enabled,
            player_scores: self.player_scores.clone(),
            current_word_list: Arc::clone(&self.current_word_list),
            local_used_words: self.local_used_words.clone(),
            game_start_time: self.game_start_time,
        }
    }
}

impl MedAndraOrdGameState {
    pub fn new(word_list_snapshot: Arc<Vec<String>>) -> Self {
        Self {
            clients: HashMap::new(),
            phase: GamePhase::Setup,
            target_points: 10,
            game_duration_seconds: 300,
            point_limit_enabled: true,
            time_limit_enabled: false,
            player_scores: HashMap::new(),
            current_word_list: word_list_snapshot,
            local_used_words: HashSet::new(),
            game_start_time: None,
        }
    }

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
                    client.id = %client_id,
                    error = %e,
                    "Failed to serialize GameEvent for client"
                );
            }
        }
    }

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
                    error = %e,
                    "Failed to serialize GameEvent for broadcast"
                );
            }
        }
    }

    async fn broadcast_full_state_update(&mut self) {
        let state_clone = self.clone();
        self.broadcast_game_event_to_all(GameEvent::FullStateUpdate(state_clone))
            .await;
    }

    async fn send_generic_message_to_client(
        &self,
        client_id: &Uuid,
        message: GenericServerToClientMessage,
    ) {
        if let Some(tx) = self.clients.get(client_id) {
            if let Ok(ws_msg) = message.to_ws_text() {
                if tx.send(ws_msg).await.is_err() {
                    tracing::warn!(
                        client.id = %client_id,
                        "Failed to send to client"
                    );
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
                    tracing::warn!(
                        client.id = %id,
                        "Failed to broadcast to client"
                    );
                }
            }
        }
    }

    pub fn check_game_time_expired(&self) -> bool {
        if !self.time_limit_enabled {
            return false;
        }

        if let Some(start_time) = self.game_start_time {
            let elapsed = start_time.elapsed();
            elapsed.as_secs() >= self.game_duration_seconds
        } else {
            false
        }
    }

    pub fn get_remaining_game_time(&self) -> Option<u64> {
        if !self.time_limit_enabled {
            return None;
        }

        if let Some(start_time) = self.game_start_time {
            let elapsed = start_time.elapsed().as_secs();
            if elapsed >= self.game_duration_seconds {
                Some(0)
            } else {
                Some(self.game_duration_seconds - elapsed)
            }
        } else {
            Some(self.game_duration_seconds)
        }
    }

    async fn end_game_time_expired(&mut self) {
        let winner = self
            .player_scores
            .iter()
            .max_by_key(|&(_, &points)| points)
            .map(|(player, _)| player.clone())
            .unwrap_or_else(|| "No players".to_string());

        self.phase = GamePhase::GameOver { winner };
        self.game_start_time = None;

        self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
            new_phase: self.phase.clone(),
        })
        .await;

        tracing::info!("Game ended due to time expiration");
    }

    async fn handle_start_game(&mut self) {
        if self.phase != GamePhase::Setup {
            return;
        }

        // Only clear player scores, not used words - preserve used words across multiple games
        self.player_scores.clear();
        self.game_start_time = Some(Instant::now());

        if let Some(word) = self.get_next_word() {
            self.phase = GamePhase::Playing {
                current_word: word.clone(),
            };
            self.broadcast_game_event_to_all(GameEvent::WordChanged {
                word,
                is_placeholder: false,
            })
            .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
        } else {
            self.phase = GamePhase::Playing {
                current_word: "Inga ord!".to_string(),
            };
            self.broadcast_game_event_to_all(GameEvent::WordChanged {
                word: "Inga ord!".to_string(),
                is_placeholder: true,
            })
            .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            tracing::warn!("No words available to start game");
        }
    }

    async fn handle_pass_word(&mut self) {
        if let GamePhase::Playing { .. } = &self.phase {
            if self.check_game_time_expired() {
                self.end_game_time_expired().await;
                return;
            }

            if let Some(word) = self.get_next_word() {
                self.phase = GamePhase::Playing {
                    current_word: word.clone(),
                };
                self.broadcast_game_event_to_all(GameEvent::WordChanged {
                    word,
                    is_placeholder: false,
                })
                .await;
            } else {
                self.phase = GamePhase::Playing {
                    current_word: "Slut på ord!".to_string(),
                };
                self.broadcast_game_event_to_all(GameEvent::WordChanged {
                    word: "Slut på ord!".to_string(),
                    is_placeholder: true,
                })
                .await;
                tracing::warn!("Ran out of words during PassWord");
            }
        }
    }

    async fn handle_reset_game(&mut self) {
        self.phase = GamePhase::Setup;
        self.player_scores.clear();
        self.local_used_words.clear();
        self.game_start_time = None;

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

    fn handle_set_game_duration(&mut self, seconds: u32) {
        if self.phase == GamePhase::Setup {
            self.game_duration_seconds = seconds as u64;
        }
    }

    fn handle_set_point_limit_enabled(&mut self, enabled: bool) {
        if self.phase == GamePhase::Setup {
            self.point_limit_enabled = enabled;
        }
    }

    fn handle_set_time_limit_enabled(&mut self, enabled: bool) {
        if self.phase == GamePhase::Setup {
            self.time_limit_enabled = enabled;
        }
    }

    fn get_next_word(&mut self) -> Option<String> {
        if self.current_word_list.is_empty() {
            tracing::warn!("Word list is empty, cannot get next word");
            return None;
        }

        let available_words: Vec<String> = self
            .current_word_list // Use current_word_list
            .iter()
            .filter(|word| !self.local_used_words.contains(*word)) // Use local_used_words
            .cloned()
            .collect();

        if available_words.is_empty() {
            tracing::info!("All words used, resetting used words list for this game");
            self.local_used_words.clear();
            // Try again with reset list
            self.current_word_list.choose(&mut thread_rng()).cloned()
        } else {
            available_words.choose(&mut thread_rng()).cloned()
        }
    }

    async fn process_correct_guess(&mut self, player: &str) {
        if self.check_game_time_expired() {
            self.end_game_time_expired().await;
            return;
        }

        let current_score = self.player_scores.entry(player.to_string()).or_insert(0);
        *current_score += 1;
        let new_score = *current_score; // Get value after update

        self.broadcast_game_event_to_all(GameEvent::PlayerScored {
            player: player.to_string(),
            points: new_score, // Use the new score
        })
        .await;

        if self.point_limit_enabled && new_score >= self.target_points {
            self.game_start_time = None;
            self.phase = GamePhase::GameOver {
                winner: player.to_string(),
            };
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            return;
        }

        if let Some(word) = self.get_next_word() {
            self.phase = GamePhase::Playing {
                current_word: word.clone(),
            };
            self.broadcast_game_event_to_all(GameEvent::WordChanged {
                word,
                is_placeholder: false,
            })
            .await;
        } else {
            self.phase = GamePhase::Playing {
                current_word: "Slut på ord!".to_string(),
            };
            self.broadcast_game_event_to_all(GameEvent::WordChanged {
                word: "Slut på ord!".to_string(),
                is_placeholder: true,
            })
            .await;
            tracing::warn!("Ran out of words after correct guess");
        }
    }

    pub async fn check_and_handle_game_expiration(&mut self) {
        if matches!(self.phase, GamePhase::Playing { .. }) && self.check_game_time_expired() {
            self.end_game_time_expired().await;
            self.broadcast_full_state_update().await;
        }
    }
}

impl GameLogic for MedAndraOrdGameState {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::debug!(
            client.id = %client_id,
            "Client connected"
        );
        self.clients.insert(client_id, client_tx);
        let state_clone = self.clone();
        self.send_game_event_to_client(&client_id, GameEvent::FullStateUpdate(state_clone))
            .await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::debug!(
            client.id = %client_id,
            "Client disconnected"
        );
        self.clients.remove(&client_id);
    }

    async fn handle_event(
        &mut self,
        _client_id: Uuid,
        message: GenericClientToServerMessage,
    ) -> EventHandlingResult {
        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
            } => {
                if game_type_id != self.game_type_id() {
                    tracing::warn!(
                        game.type_id = %game_type_id,
                        "Wrong game_type_id"
                    );
                    return EventHandlingResult::Handled;
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
                            AdminCommand::SetGameDuration { seconds } => {
                                self.handle_set_game_duration(seconds)
                            }
                            AdminCommand::SetPointLimitEnabled { enabled } => {
                                self.handle_set_point_limit_enabled(enabled)
                            }
                            AdminCommand::SetTimeLimitEnabled { enabled } => {
                                self.handle_set_time_limit_enabled(enabled)
                            }
                        }
                        self.broadcast_full_state_update().await;
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "Failed to deserialize command"
                        );
                    }
                }
            }
            GenericClientToServerMessage::LeaveLobby => {
                tracing::info!(
                    client.id = %_client_id,
                    "Client explicitly leaving lobby"
                );
                return EventHandlingResult::DisconnectClient;
            }
            GenericClientToServerMessage::GlobalCommand { .. } => {
                tracing::trace!("Received GlobalCommand (unhandled)");
            }
            _ => {
                tracing::warn!("Unrecognized message type");
            }
        }
        EventHandlingResult::Handled
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        if let GamePhase::Playing { current_word } = &self.phase {
            if self.check_game_time_expired() {
                self.end_game_time_expired().await;
                self.broadcast_full_state_update().await;
                return;
            }

            if self.current_word_list.is_empty() {
                return;
            }

            let guess = message.text.trim();

            if is_guess_acceptable(current_word, guess) {
                tracing::debug!(
                    guess = %guess,
                    player = %message.sender_username,
                    "Correct guess"
                );
                self.local_used_words.insert(current_word.clone()); // Use local_used_words
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
