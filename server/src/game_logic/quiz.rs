use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::db::{TrivialPursuitData, TrivialPursuitQuestion};
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::game_logic::utils::is_guess_acceptable;
use crate::game_logic::{EventHandlingResult, GameLogic};
use crate::twitch::ParsedTwitchMessage;

const GAME_TYPE_ID_QUIZ: &str = "Quiz";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecentGuess {
    pub id: String,
    pub player: String,
    pub guessed_text: String,
    pub correct_answer: String,
    pub question: String,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
    PassQuestion,
    ResetGame,
    SetTargetPoints { points: u32 },
    SetGameDuration { seconds: u32 },
    SetPointLimitEnabled { enabled: bool },
    SetTimeLimitEnabled { enabled: bool },
    RemoveRecentGuess { guess_id: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    QuestionChanged {
        question: String,
        is_placeholder: bool,
    },
    PlayerScored {
        player: String,
        points: u32,
    },
    GamePhaseChanged {
        new_phase: GamePhase,
    },
    GameTimeUpdate {
        remaining_seconds: u64,
    },
    RecentGuessesUpdated {
        recent_guesses: Vec<RecentGuess>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum GamePhase {
    Setup,
    Playing {
        current_question: String,
        current_answer: String,
        extra_info: Option<String>,
    },
    GameOver {
        winner: String,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QuizGameState {
    #[serde(skip)]
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,

    pub phase: GamePhase,
    pub target_points: u32,
    pub game_duration_seconds: u64,
    pub point_limit_enabled: bool,
    pub time_limit_enabled: bool,
    pub player_scores: HashMap<String, u32>,
    pub recent_guesses: Vec<RecentGuess>,

    #[serde(skip)]
    trivial_pursuit_data: Option<Arc<TrivialPursuitData>>,
    #[serde(skip)]
    local_used_question_ids: HashSet<u32>,
    #[serde(skip)]
    game_start_time: Option<Instant>,
}

impl Clone for QuizGameState {
    fn clone(&self) -> Self {
        Self {
            clients: HashMap::new(),
            phase: self.phase.clone(),
            target_points: self.target_points,
            game_duration_seconds: self.game_duration_seconds,
            point_limit_enabled: self.point_limit_enabled,
            time_limit_enabled: self.time_limit_enabled,
            player_scores: self.player_scores.clone(),
            recent_guesses: self.recent_guesses.clone(),
            trivial_pursuit_data: self.trivial_pursuit_data.clone(),
            local_used_question_ids: self.local_used_question_ids.clone(),
            game_start_time: self.game_start_time,
        }
    }
}

impl QuizGameState {
    pub fn new(trivial_pursuit_data: Option<Arc<TrivialPursuitData>>) -> Self {
        Self {
            clients: HashMap::new(),
            phase: GamePhase::Setup,
            target_points: 10,
            game_duration_seconds: 300,
            point_limit_enabled: true,
            time_limit_enabled: false,
            player_scores: HashMap::new(),
            recent_guesses: Vec::new(),
            trivial_pursuit_data,
            local_used_question_ids: HashSet::new(),
            game_start_time: None,
        }
    }

    async fn broadcast_game_event_to_all(&self, event_payload: GameEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_QUIZ.to_string(),
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
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_QUIZ.to_string(),
            &serde_json::json!({
                "event_type": "FullStateUpdate",
                "data": state_clone
            }),
        ) {
            Ok(wrapped_message) => {
                self.broadcast_generic_message_to_all(wrapped_message).await;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Failed to serialize FullStateUpdate for broadcast"
                );
            }
        }
    }

    async fn send_full_state_to_client(&self, client_id: &Uuid, state: &QuizGameState) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_QUIZ.to_string(),
            &serde_json::json!({
                "event_type": "FullStateUpdate",
                "data": state
            }),
        ) {
            Ok(wrapped_message) => {
                self.send_generic_message_to_client(client_id, wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    client.id = %client_id,
                    "Failed to serialize FullStateUpdate for client"
                );
            }
        }
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

        // Only clear player scores, not used question IDs - preserve used questions across multiple games
        self.player_scores.clear();
        self.game_start_time = Some(Instant::now());

        if let Some((question, answer, extra_info)) = self.get_next_question() {
            self.phase = GamePhase::Playing {
                current_question: question.clone(),
                current_answer: answer,
                extra_info,
            };
            self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                question,
                is_placeholder: false,
            })
            .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
        } else {
            self.phase = GamePhase::Playing {
                current_question: "No questions!".to_string(),
                current_answer: "".to_string(),
                extra_info: None,
            };
            self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                question: "No questions!".to_string(),
                is_placeholder: true,
            })
            .await;
            self.broadcast_game_event_to_all(GameEvent::GamePhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            tracing::warn!("No questions available to start game");
        }
    }

    async fn handle_pass_question(&mut self) {
        if let GamePhase::Playing { .. } = &self.phase {
            if self.check_game_time_expired() {
                self.end_game_time_expired().await;
                return;
            }

            if let Some((question, answer, extra_info)) = self.get_next_question() {
                self.phase = GamePhase::Playing {
                    current_question: question.clone(),
                    current_answer: answer,
                    extra_info,
                };
                self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                    question,
                    is_placeholder: false,
                })
                .await;
            } else {
                self.phase = GamePhase::Playing {
                    current_question: "Out of questions!".to_string(),
                    current_answer: "".to_string(),
                    extra_info: None,
                };
                self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                    question: "Out of questions!".to_string(),
                    is_placeholder: true,
                })
                .await;
                tracing::warn!("Ran out of questions during PassQuestion");
            }
        }
    }

    async fn handle_reset_game(&mut self) {
        self.phase = GamePhase::Setup;
        self.player_scores.clear();
        self.local_used_question_ids.clear();
        self.recent_guesses.clear();
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

    /// Adds a correct guess to the recent guesses list, maintaining a maximum of 5 entries.
    fn add_recent_guess(
        &mut self,
        player: &str,
        guessed_text: &str,
        correct_answer: &str,
        question: &str,
    ) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let guess = RecentGuess {
            id: uuid::Uuid::new_v4().to_string(),
            player: player.to_string(),
            guessed_text: guessed_text.to_string(),
            correct_answer: correct_answer.to_string(),
            question: question.to_string(),
            timestamp,
        };

        // Add to front of list
        self.recent_guesses.insert(0, guess);

        // Keep only the 5 most recent
        if self.recent_guesses.len() > 5 {
            self.recent_guesses.truncate(5);
        }
    }

    /// Removes a recent guess by ID and deducts one point from the player.
    async fn handle_remove_recent_guess(&mut self, guess_id: &str) {
        if let Some(pos) = self.recent_guesses.iter().position(|g| g.id == guess_id) {
            let removed_guess = self.recent_guesses.remove(pos);

            // Deduct point from player
            if let Some(current_score) = self.player_scores.get_mut(&removed_guess.player) {
                if *current_score > 0 {
                    *current_score -= 1;

                    tracing::info!(
                        player = %removed_guess.player,
                        guess = %removed_guess.guessed_text,
                        answer = %removed_guess.correct_answer,
                        question = %removed_guess.question,
                        new_score = *current_score,
                        "Removed recent guess and deducted point"
                    );
                }
            }

            self.broadcast_game_event_to_all(GameEvent::RecentGuessesUpdated {
                recent_guesses: self.recent_guesses.clone(),
            })
            .await;
        }
    }

    fn get_next_question(&mut self) -> Option<(String, String, Option<String>)> {
        let trivial_pursuit_data = self.trivial_pursuit_data.as_ref()?;

        if trivial_pursuit_data.cards.is_empty() {
            tracing::warn!("No Trivial Pursuit cards available");
            return None;
        }

        // Collect all available questions
        let mut available_questions: Vec<&TrivialPursuitQuestion> = Vec::new();
        for card in &trivial_pursuit_data.cards {
            for question in &card.questions {
                if !self.local_used_question_ids.contains(&question.id) {
                    available_questions.push(question);
                }
            }
        }

        if available_questions.is_empty() {
            tracing::info!("All questions used, resetting used questions list for this game");
            self.local_used_question_ids.clear();
            // Try again with reset list - get all questions
            for card in &trivial_pursuit_data.cards {
                for question in &card.questions {
                    available_questions.push(question);
                }
            }
        }

        if let Some(selected_question) = available_questions.choose(&mut thread_rng()) {
            Some((
                selected_question.question.clone(),
                selected_question.answer.clone(),
                selected_question.extra_info.clone(),
            ))
        } else {
            None
        }
    }

    async fn process_correct_guess(
        &mut self,
        player: &str,
        guessed_text: &str,
        correct_answer: &str,
        question: &str,
        question_id: u32,
    ) {
        if self.check_game_time_expired() {
            self.end_game_time_expired().await;
            return;
        }

        let current_score = self.player_scores.entry(player.to_string()).or_insert(0);
        *current_score += 1;
        let new_score = *current_score;

        self.add_recent_guess(player, guessed_text, correct_answer, question);

        // Mark question as used
        self.local_used_question_ids.insert(question_id);

        self.broadcast_game_event_to_all(GameEvent::PlayerScored {
            player: player.to_string(),
            points: new_score,
        })
        .await;

        self.broadcast_game_event_to_all(GameEvent::RecentGuessesUpdated {
            recent_guesses: self.recent_guesses.clone(),
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

        if let Some((question, answer, extra_info)) = self.get_next_question() {
            self.phase = GamePhase::Playing {
                current_question: question.clone(),
                current_answer: answer,
                extra_info,
            };
            self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                question,
                is_placeholder: false,
            })
            .await;
        } else {
            self.phase = GamePhase::Playing {
                current_question: "Out of questions!".to_string(),
                current_answer: "".to_string(),
                extra_info: None,
            };
            self.broadcast_game_event_to_all(GameEvent::QuestionChanged {
                question: "Out of questions!".to_string(),
                is_placeholder: true,
            })
            .await;
            tracing::warn!("Ran out of questions after correct guess");
        }
    }
}

impl GameLogic for QuizGameState {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::debug!(
            client.id = %client_id,
            "Client connected"
        );
        self.clients.insert(client_id, client_tx);
        self.send_full_state_to_client(&client_id, self).await;
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
                            AdminCommand::PassQuestion => self.handle_pass_question().await,
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
                            AdminCommand::RemoveRecentGuess { guess_id } => {
                                self.handle_remove_recent_guess(&guess_id).await
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
        if let GamePhase::Playing {
            current_question: _,
            current_answer,
            extra_info: _,
        } = &self.phase
        {
            if self.check_game_time_expired() {
                self.end_game_time_expired().await;
                self.broadcast_full_state_update().await;
                return;
            }

            let trivial_pursuit_data = match &self.trivial_pursuit_data {
                Some(data) => data,
                None => return,
            };

            let guess = message.text.trim();
            let answer = current_answer.clone();

            // Find the current question ID to mark it as used
            let mut question_id = None;
            'outer: for card in &trivial_pursuit_data.cards {
                for question in &card.questions {
                    if question.answer == answer {
                        question_id = Some(question.id);
                        break 'outer;
                    }
                }
            }

            if is_guess_acceptable(&answer, guess) {
                tracing::debug!(
                    guess = %guess,
                    answer = %answer,
                    player = %message.sender_username,
                    "Correct guess"
                );

                if let Some(qid) = question_id {
                    if let GamePhase::Playing {
                        current_question, ..
                    } = &self.phase
                    {
                        let question = current_question.clone();
                        self.process_correct_guess(
                            &message.sender_username,
                            guess,
                            &answer,
                            &question,
                            qid,
                        )
                        .await;
                        self.broadcast_full_state_update().await;
                    }
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_QUIZ.to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }

    fn get_all_client_ids(&self) -> Vec<Uuid> {
        self.clients.keys().copied().collect()
    }
}
