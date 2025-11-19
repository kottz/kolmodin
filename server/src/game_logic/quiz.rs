use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::content::{TrivialPursuitData, VemVetMestQuestion};
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
pub enum QuizAdminCommand {
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
pub enum QuizEvent {
    QuestionChanged {
        question: String,
        is_placeholder: bool,
    },
    PlayerScored {
        player: String,
        points: u32,
    },
    QuizPhaseChanged {
        new_phase: QuizPhase,
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
pub enum QuizPhase {
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
pub struct QuizGame {
    #[serde(skip)]
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,

    pub phase: QuizPhase,
    pub target_points: u32,
    pub game_duration_seconds: u64,
    pub point_limit_enabled: bool,
    pub time_limit_enabled: bool,
    pub player_scores: HashMap<String, u32>,
    pub recent_guesses: Vec<RecentGuess>,

    #[serde(skip)]
    trivial_pursuit_data: Option<Arc<TrivialPursuitData>>,
    #[serde(skip)]
    vem_vet_mest_data: Option<Arc<Vec<VemVetMestQuestion>>>,
    #[serde(skip)]
    local_used_question_ids: HashSet<u32>,
    #[serde(skip)]
    local_used_vem_vet_mest_indices: HashSet<usize>,
    #[serde(skip)]
    game_start_time: Option<Instant>,
}

impl Clone for QuizGame {
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
            vem_vet_mest_data: self.vem_vet_mest_data.clone(),
            local_used_question_ids: self.local_used_question_ids.clone(),
            local_used_vem_vet_mest_indices: self.local_used_vem_vet_mest_indices.clone(),
            game_start_time: self.game_start_time,
        }
    }
}

impl QuizGame {
    pub fn new(
        trivial_pursuit_data: Option<Arc<TrivialPursuitData>>,
        vem_vet_mest_data: Option<Arc<Vec<VemVetMestQuestion>>>,
    ) -> Self {
        Self {
            clients: HashMap::new(),
            phase: QuizPhase::Setup,
            target_points: 10,
            game_duration_seconds: 300,
            point_limit_enabled: true,
            time_limit_enabled: false,
            player_scores: HashMap::new(),
            recent_guesses: Vec::new(),
            trivial_pursuit_data,
            vem_vet_mest_data,
            local_used_question_ids: HashSet::new(),
            local_used_vem_vet_mest_indices: HashSet::new(),
            game_start_time: None,
        }
    }

    async fn broadcast_game_event_to_all(&self, event_payload: QuizEvent) {
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
                    "Failed to serialize QuizEvent for broadcast"
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

    async fn send_full_state_to_client(&self, client_id: &Uuid, state: &QuizGame) {
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

        self.phase = QuizPhase::GameOver { winner };
        self.game_start_time = None;

        self.broadcast_game_event_to_all(QuizEvent::QuizPhaseChanged {
            new_phase: self.phase.clone(),
        })
        .await;

        tracing::info!("Game ended due to time expiration");
    }

    async fn handle_start_game(&mut self) {
        if self.phase != QuizPhase::Setup {
            return;
        }

        // Only clear player scores, not used question IDs - preserve used questions across multiple games
        self.player_scores.clear();
        self.game_start_time = Some(Instant::now());

        if let Some((question, answer, extra_info)) = self.get_next_question() {
            self.phase = QuizPhase::Playing {
                current_question: question.clone(),
                current_answer: answer,
                extra_info,
            };
            self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                question,
                is_placeholder: false,
            })
            .await;
            self.broadcast_game_event_to_all(QuizEvent::QuizPhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
        } else {
            self.phase = QuizPhase::Playing {
                current_question: "No questions!".to_string(),
                current_answer: "".to_string(),
                extra_info: None,
            };
            self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                question: "No questions!".to_string(),
                is_placeholder: true,
            })
            .await;
            self.broadcast_game_event_to_all(QuizEvent::QuizPhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            tracing::warn!("No questions available to start game");
        }
    }

    async fn handle_pass_question(&mut self) {
        if let QuizPhase::Playing { .. } = &self.phase {
            if self.check_game_time_expired() {
                self.end_game_time_expired().await;
                return;
            }

            if let Some((question, answer, extra_info)) = self.get_next_question() {
                self.phase = QuizPhase::Playing {
                    current_question: question.clone(),
                    current_answer: answer,
                    extra_info,
                };
                self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                    question,
                    is_placeholder: false,
                })
                .await;
            } else {
                self.phase = QuizPhase::Playing {
                    current_question: "Out of questions!".to_string(),
                    current_answer: "".to_string(),
                    extra_info: None,
                };
                self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                    question: "Out of questions!".to_string(),
                    is_placeholder: true,
                })
                .await;
                tracing::warn!("Ran out of questions during PassQuestion");
            }
        }
    }

    async fn handle_reset_game(&mut self) {
        self.phase = QuizPhase::Setup;
        self.player_scores.clear();
        self.local_used_question_ids.clear();
        self.local_used_vem_vet_mest_indices.clear();
        self.recent_guesses.clear();
        self.game_start_time = None;

        self.broadcast_game_event_to_all(QuizEvent::QuizPhaseChanged {
            new_phase: self.phase.clone(),
        })
        .await;
    }

    fn handle_set_target_points(&mut self, points: u32) {
        if self.phase == QuizPhase::Setup {
            self.target_points = points;
        }
    }

    fn handle_set_game_duration(&mut self, seconds: u32) {
        if self.phase == QuizPhase::Setup {
            self.game_duration_seconds = seconds as u64;
        }
    }

    fn handle_set_point_limit_enabled(&mut self, enabled: bool) {
        if self.phase == QuizPhase::Setup {
            self.point_limit_enabled = enabled;
        }
    }

    fn handle_set_time_limit_enabled(&mut self, enabled: bool) {
        if self.phase == QuizPhase::Setup {
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

            self.broadcast_game_event_to_all(QuizEvent::RecentGuessesUpdated {
                recent_guesses: self.recent_guesses.clone(),
            })
            .await;
        }
    }

    fn get_next_question(&mut self) -> Option<(String, String, Option<String>)> {
        // Collect available questions from both sources
        enum QuestionSource {
            TrivialPursuit(u32, u32), // (card_id, question_id) - compound key to handle duplicate question IDs across cards
            VemVetMest(usize),        // index in the array
        }

        let mut available_sources: Vec<QuestionSource> = Vec::new();

        // Add available Trivial Pursuit questions using compound key (card_id, question_id)
        if let Some(trivial_pursuit_data) = &self.trivial_pursuit_data {
            for card in &trivial_pursuit_data.cards {
                for question in &card.questions {
                    let compound_key = card.id * 1000 + question.id; // Create unique compound key
                    if !self.local_used_question_ids.contains(&compound_key) {
                        available_sources
                            .push(QuestionSource::TrivialPursuit(card.id, question.id));
                    }
                }
            }
        }

        // Add available Vem Vet Mest questions
        if let Some(vem_vet_mest_data) = &self.vem_vet_mest_data {
            for (index, _question) in vem_vet_mest_data.iter().enumerate() {
                if !self.local_used_vem_vet_mest_indices.contains(&index) {
                    available_sources.push(QuestionSource::VemVetMest(index));
                }
            }
        }

        // If no questions available, reset both used sets and try again
        if available_sources.is_empty() {
            tracing::info!("All questions from both sources used, resetting used questions lists");
            self.local_used_question_ids.clear();
            self.local_used_vem_vet_mest_indices.clear();

            // Rebuild available sources
            if let Some(trivial_pursuit_data) = &self.trivial_pursuit_data {
                for card in &trivial_pursuit_data.cards {
                    for question in &card.questions {
                        available_sources
                            .push(QuestionSource::TrivialPursuit(card.id, question.id));
                    }
                }
            }

            if let Some(vem_vet_mest_data) = &self.vem_vet_mest_data {
                for (index, _question) in vem_vet_mest_data.iter().enumerate() {
                    available_sources.push(QuestionSource::VemVetMest(index));
                }
            }
        }

        // Randomly select a question source
        if let Some(selected_source) = available_sources.choose(&mut thread_rng()) {
            match selected_source {
                QuestionSource::TrivialPursuit(card_id, question_id) => {
                    // Find and return the specific Trivial Pursuit question using both card and question ID
                    if let Some(trivial_pursuit_data) = &self.trivial_pursuit_data {
                        for card in &trivial_pursuit_data.cards {
                            if card.id == *card_id {
                                for question in &card.questions {
                                    if question.id == *question_id {
                                        return Some((
                                            question.question.clone(),
                                            question.answer.clone(),
                                            question.extra_info.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                QuestionSource::VemVetMest(index) => {
                    // Return the Vem Vet Mest question
                    if let Some(vem_vet_mest_data) = &self.vem_vet_mest_data {
                        if let Some(question) = vem_vet_mest_data.get(*index) {
                            return Some((
                                question.question.clone(),
                                question.answer.clone(),
                                question.extra_info.clone(),
                            ));
                        }
                    }
                }
            }
        }

        None
    }

    fn mark_question_as_used(&mut self, question: &str, answer: &str) {
        // Try to find and mark in Trivial Pursuit data using compound key
        if let Some(trivial_pursuit_data) = &self.trivial_pursuit_data {
            for card in &trivial_pursuit_data.cards {
                for tp_question in &card.questions {
                    if tp_question.question == question && tp_question.answer == answer {
                        let compound_key = card.id * 1000 + tp_question.id; // Same compound key logic as get_next_question
                        self.local_used_question_ids.insert(compound_key);
                        tracing::debug!(
                            card_id = card.id,
                            question_id = tp_question.id,
                            compound_key = compound_key,
                            "Marked Trivial Pursuit question as used"
                        );
                        return;
                    }
                }
            }
        }

        // Try to find and mark in Vem Vet Mest data
        if let Some(vem_vet_mest_data) = &self.vem_vet_mest_data {
            for (index, vvm_question) in vem_vet_mest_data.iter().enumerate() {
                if vvm_question.question == question && vvm_question.answer == answer {
                    self.local_used_vem_vet_mest_indices.insert(index);
                    tracing::debug!(index = index, "Marked Vem Vet Mest question as used");
                    return;
                }
            }
        }

        tracing::warn!("Could not find question to mark as used: '{}'", question);
    }

    async fn process_correct_guess(
        &mut self,
        player: &str,
        guessed_text: &str,
        correct_answer: &str,
        question: &str,
    ) {
        if self.check_game_time_expired() {
            self.end_game_time_expired().await;
            return;
        }

        let current_score = self.player_scores.entry(player.to_string()).or_insert(0);
        *current_score += 1;
        let new_score = *current_score;

        self.add_recent_guess(player, guessed_text, correct_answer, question);

        // Mark question as used - find it in both sources by matching answer and question
        self.mark_question_as_used(question, correct_answer);

        self.broadcast_game_event_to_all(QuizEvent::PlayerScored {
            player: player.to_string(),
            points: new_score,
        })
        .await;

        self.broadcast_game_event_to_all(QuizEvent::RecentGuessesUpdated {
            recent_guesses: self.recent_guesses.clone(),
        })
        .await;

        if self.point_limit_enabled && new_score >= self.target_points {
            self.game_start_time = None;
            self.phase = QuizPhase::GameOver {
                winner: player.to_string(),
            };
            self.broadcast_game_event_to_all(QuizEvent::QuizPhaseChanged {
                new_phase: self.phase.clone(),
            })
            .await;
            return;
        }

        if let Some((question, answer, extra_info)) = self.get_next_question() {
            self.phase = QuizPhase::Playing {
                current_question: question.clone(),
                current_answer: answer,
                extra_info,
            };
            self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                question,
                is_placeholder: false,
            })
            .await;
        } else {
            self.phase = QuizPhase::Playing {
                current_question: "Out of questions!".to_string(),
                current_answer: "".to_string(),
                extra_info: None,
            };
            self.broadcast_game_event_to_all(QuizEvent::QuestionChanged {
                question: "Out of questions!".to_string(),
                is_placeholder: true,
            })
            .await;
            tracing::warn!("Ran out of questions after correct guess");
        }
    }
}

impl GameLogic for QuizGame {
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

                match serde_json::from_value::<QuizAdminCommand>(command_data) {
                    Ok(cmd) => {
                        match cmd {
                            QuizAdminCommand::StartGame => self.handle_start_game().await,
                            QuizAdminCommand::PassQuestion => self.handle_pass_question().await,
                            QuizAdminCommand::ResetGame => self.handle_reset_game().await,
                            QuizAdminCommand::SetTargetPoints { points } => {
                                self.handle_set_target_points(points)
                            }
                            QuizAdminCommand::SetGameDuration { seconds } => {
                                self.handle_set_game_duration(seconds)
                            }
                            QuizAdminCommand::SetPointLimitEnabled { enabled } => {
                                self.handle_set_point_limit_enabled(enabled)
                            }
                            QuizAdminCommand::SetTimeLimitEnabled { enabled } => {
                                self.handle_set_time_limit_enabled(enabled)
                            }
                            QuizAdminCommand::RemoveRecentGuess { guess_id } => {
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
        if let QuizPhase::Playing {
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

            let guess = message.text.trim();
            let answer = current_answer.clone();

            if is_guess_acceptable(&answer, guess) {
                tracing::debug!(
                    guess = %guess,
                    answer = %answer,
                    player = %message.sender_username,
                    "Correct guess"
                );

                if let QuizPhase::Playing {
                    current_question, ..
                } = &self.phase
                {
                    let question = current_question.clone();
                    self.process_correct_guess(&message.sender_username, guess, &answer, &question)
                        .await;
                    self.broadcast_full_state_update().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{TrivialPursuitCard, TrivialPursuitQuestion};

    #[test]
    fn test_quiz_with_both_sources() {
        // Create test Trivial Pursuit data
        let tp_question = TrivialPursuitQuestion {
            id: 1,
            question: "What is 2+2?".to_string(),
            answer: "4".to_string(),
            extra_info: None,
        };
        let tp_card = TrivialPursuitCard {
            id: 1,
            questions: vec![tp_question],
        };
        let tp_data = Arc::new(TrivialPursuitData {
            cards: vec![tp_card],
        });

        // Create test Vem Vet Mest data
        let vvm_question = VemVetMestQuestion {
            question: "What is the capital of Sweden?".to_string(),
            answer: "Stockholm".to_string(),
            category: Some("Geography".to_string()),
            extra_info: None,
        };
        let vvm_data = Arc::new(vec![vvm_question]);

        // Create quiz state with both sources
        let mut quiz_state = QuizGame::new(Some(tp_data), Some(vvm_data));

        // Get first question - should randomly pick from either source
        let first_question = quiz_state.get_next_question();
        assert!(first_question.is_some());

        let (question, answer, _extra_info) = first_question.unwrap();

        // Should be one of our test questions
        assert!(
            (question == "What is 2+2?" && answer == "4")
                || (question == "What is the capital of Sweden?" && answer == "Stockholm")
        );

        // Mark the question as used
        quiz_state.mark_question_as_used(&question, &answer);

        // Get second question - should get the other one
        let second_question = quiz_state.get_next_question();
        assert!(second_question.is_some());

        let (question2, answer2, _extra_info2) = second_question.unwrap();

        // Should be the other question
        assert!(question != question2);
        assert!(answer != answer2);

        // Should be one of our test questions
        assert!(
            (question2 == "What is 2+2?" && answer2 == "4")
                || (question2 == "What is the capital of Sweden?" && answer2 == "Stockholm")
        );
    }

    #[test]
    fn test_quiz_with_only_trivial_pursuit() {
        // Create test Trivial Pursuit data
        let tp_question = TrivialPursuitQuestion {
            id: 1,
            question: "What is 2+2?".to_string(),
            answer: "4".to_string(),
            extra_info: None,
        };
        let tp_card = TrivialPursuitCard {
            id: 1,
            questions: vec![tp_question],
        };
        let tp_data = Arc::new(TrivialPursuitData {
            cards: vec![tp_card],
        });

        // Create quiz state with only TP data
        let mut quiz_state = QuizGame::new(Some(tp_data), None);

        // Should get the TP question
        let question_result = quiz_state.get_next_question();
        assert!(question_result.is_some());

        let (question, answer, _extra_info) = question_result.unwrap();
        assert_eq!(question, "What is 2+2?");
        assert_eq!(answer, "4");
    }

    #[test]
    fn test_quiz_with_only_vem_vet_mest() {
        // Create test Vem Vet Mest data
        let vvm_question = VemVetMestQuestion {
            question: "What is the capital of Sweden?".to_string(),
            answer: "Stockholm".to_string(),
            category: Some("Geography".to_string()),
            extra_info: None,
        };
        let vvm_data = Arc::new(vec![vvm_question]);

        // Create quiz state with only VVM data
        let mut quiz_state = QuizGame::new(None, Some(vvm_data));

        // Should get the VVM question
        let question_result = quiz_state.get_next_question();
        assert!(question_result.is_some());

        let (question, answer, _extra_info) = question_result.unwrap();
        assert_eq!(question, "What is the capital of Sweden?");
        assert_eq!(answer, "Stockholm");
    }

    #[test]
    fn test_quiz_handles_duplicate_question_ids_across_cards() {
        // Create test data with two cards that have questions with the same IDs (1 and 2)
        // This simulates the real-world scenario where each card has questions 1-6
        let card1_q1 = TrivialPursuitQuestion {
            id: 1,
            question: "Card 1 Question 1".to_string(),
            answer: "Card 1 Answer 1".to_string(),
            extra_info: None,
        };
        let card1_q2 = TrivialPursuitQuestion {
            id: 2,
            question: "Card 1 Question 2".to_string(),
            answer: "Card 1 Answer 2".to_string(),
            extra_info: None,
        };
        let card1 = TrivialPursuitCard {
            id: 1,
            questions: vec![card1_q1, card1_q2],
        };

        let card2_q1 = TrivialPursuitQuestion {
            id: 1, // Same ID as card1_q1, but different card
            question: "Card 2 Question 1".to_string(),
            answer: "Card 2 Answer 1".to_string(),
            extra_info: None,
        };
        let card2_q2 = TrivialPursuitQuestion {
            id: 2, // Same ID as card1_q2, but different card
            question: "Card 2 Question 2".to_string(),
            answer: "Card 2 Answer 2".to_string(),
            extra_info: None,
        };
        let card2 = TrivialPursuitCard {
            id: 2,
            questions: vec![card2_q1, card2_q2],
        };

        let tp_data = Arc::new(TrivialPursuitData {
            cards: vec![card1, card2],
        });

        let mut quiz_state = QuizGame::new(Some(tp_data), None);

        // Get 4 questions and verify they are all unique
        let mut questions_seen = std::collections::HashSet::new();

        for i in 0..4 {
            let question_result = quiz_state.get_next_question();
            assert!(
                question_result.is_some(),
                "Question {} should be available",
                i + 1
            );

            let (question, answer, _extra_info) = question_result.unwrap();

            // Verify this is a unique question we haven't seen before
            assert!(
                questions_seen.insert(question.clone()),
                "Question '{}' was already seen, indicating duplicate question selection bug",
                question
            );

            // Mark the question as used
            quiz_state.mark_question_as_used(&question, &answer);
        }

        // Verify all 4 questions were unique and from both cards
        assert_eq!(questions_seen.len(), 4);
        assert!(questions_seen.contains("Card 1 Question 1"));
        assert!(questions_seen.contains("Card 1 Question 2"));
        assert!(questions_seen.contains("Card 2 Question 1"));
        assert!(questions_seen.contains("Card 2 Question 2"));

        // Verify no more questions are available (all used)
        let no_more_questions = quiz_state.get_next_question();
        assert!(
            no_more_questions.is_some(),
            "Should reset and provide questions again when all are used"
        );
    }
}
