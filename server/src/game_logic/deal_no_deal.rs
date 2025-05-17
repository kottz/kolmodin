// src/game_logic/deal_no_deal.rs
use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::game_logic::GameLogic;
use crate::twitch_integration::ParsedTwitchMessage;

const GAME_TYPE_ID_DND: &str = "DealNoDeal";

const TOTAL_CASES: u8 = 22;
const MONEY_VALUES: [u64; TOTAL_CASES as usize] = [
    1, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1_000, 5_000, 10_000, 25_000, 50_000,
    75_000, 100_000, 250_000, 500_000, 1_000_000,
];
const ROUND_SCHEDULE: [u8; 6] = [6, 5, 4, 3, 2, 1];

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
    StartVoting,
    ConcludeVotingAndProcess,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    FullStateUpdate(DealNoDealGame),
    PlayerVoteRegistered {
        // NEW: Lightweight event for individual valid votes
        voter_username: String,
        vote_value: String, // The parsed, validated vote (e.g., "15", "DEAL")
    },
    // VoteTallyUpdate is removed; tally is part of FullStateUpdate
    CaseOpened {
        case_index: usize,
        value: u64,
        is_player_case_reveal_at_end: bool,
    },
    BankerOfferPresented {
        offer_amount: u64,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum GamePhase {
    Setup,
    PlayerCaseSelection_AwaitVoteStart,
    PlayerCaseSelection_Voting,
    RoundCaseOpening_AwaitVoteStart {
        round_number: u8,
        total_to_open_for_round: u8,
        opened_so_far_for_round: u8,
    },
    RoundCaseOpening_Voting {
        round_number: u8,
        total_to_open_for_round: u8,
        opened_so_far_for_round: u8,
    },
    BankerOfferCalculation {
        round_number: u8,
    },
    DealOrNoDeal_AwaitVoteStart {
        round_number: u8,
        offer: u64,
    },
    DealOrNoDeal_Voting {
        round_number: u8,
        offer: u64,
    },
    GameOver {
        summary: String,
        winnings: u64,
        player_case_original_value: u64,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DealNoDealGame {
    #[serde(skip)]
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
    pub phase: GamePhase,
    pub briefcase_values: Vec<u64>,
    pub briefcase_is_opened: Vec<bool>,
    pub player_chosen_case_index: Option<usize>,
    pub remaining_money_values_in_play: Vec<u64>,
    pub current_round_schedule_index: usize,
    current_votes_by_user: HashMap<String, String>, // Kept for internal tallying logic

    // Derived fields, populated by prepare_for_client_view
    pub current_round_display_number: u8,
    pub cases_to_open_this_round_target: u8,
    pub cases_opened_in_current_round_segment: u8,
    pub banker_offer: Option<u64>,
    pub current_vote_tally: Option<HashMap<String, u32>>, // This is the full tally
}

impl DealNoDealGame {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            phase: GamePhase::Setup,
            briefcase_values: Vec::new(),
            briefcase_is_opened: Vec::new(),
            player_chosen_case_index: None,
            remaining_money_values_in_play: Vec::new(),
            current_round_schedule_index: 0,
            current_votes_by_user: HashMap::new(),
            current_round_display_number: 0,
            cases_to_open_this_round_target: 0,
            cases_opened_in_current_round_segment: 0,
            banker_offer: None,
            current_vote_tally: None,
        }
    }

    fn prepare_for_client_view(&mut self) {
        let (round_num, target, opened_segment, tally_opt, offer) = match &self.phase {
            GamePhase::Setup | GamePhase::PlayerCaseSelection_AwaitVoteStart => {
                (0, 0, 0, None, None)
            }
            GamePhase::PlayerCaseSelection_Voting => {
                (0, 0, 0, Some(self.tally_current_votes_internal()), None)
            } // Use internal tally
            GamePhase::RoundCaseOpening_AwaitVoteStart {
                round_number,
                total_to_open_for_round,
                opened_so_far_for_round,
            } => (
                *round_number,
                *total_to_open_for_round,
                *opened_so_far_for_round,
                None,
                None,
            ),
            GamePhase::RoundCaseOpening_Voting {
                round_number,
                total_to_open_for_round,
                opened_so_far_for_round,
            } => (
                *round_number,
                *total_to_open_for_round,
                *opened_so_far_for_round,
                Some(self.tally_current_votes_internal()),
                None,
            ),
            GamePhase::BankerOfferCalculation { round_number } => (*round_number, 0, 0, None, None),
            GamePhase::DealOrNoDeal_AwaitVoteStart {
                round_number,
                offer: current_offer,
            } => {
                let prev_round_target = if self.current_round_schedule_index > 0
                    && self.current_round_schedule_index <= ROUND_SCHEDULE.len()
                {
                    ROUND_SCHEDULE[self.current_round_schedule_index - 1]
                } else {
                    0
                };
                (
                    *round_number,
                    prev_round_target,
                    prev_round_target,
                    None,
                    Some(*current_offer),
                )
            }
            GamePhase::DealOrNoDeal_Voting {
                round_number,
                offer: current_offer,
            } => {
                let prev_round_target = if self.current_round_schedule_index > 0
                    && self.current_round_schedule_index <= ROUND_SCHEDULE.len()
                {
                    ROUND_SCHEDULE[self.current_round_schedule_index - 1]
                } else {
                    0
                };
                (
                    *round_number,
                    prev_round_target,
                    prev_round_target,
                    Some(self.tally_current_votes_internal()),
                    Some(*current_offer),
                )
            }
            GamePhase::GameOver { winnings, .. } => {
                (ROUND_SCHEDULE.len() as u8 + 1, 0, 0, None, Some(*winnings))
            }
        };
        self.current_round_display_number = round_num;
        self.cases_to_open_this_round_target = target;
        self.cases_opened_in_current_round_segment = opened_segment;
        self.current_vote_tally = tally_opt;
        self.banker_offer = offer;
    }

    async fn send_game_event_to_client(&self, client_id: &Uuid, event_payload: GameEvent) {
        let event_to_send = match event_payload {
            GameEvent::FullStateUpdate(mut state_for_client) => {
                state_for_client.prepare_for_client_view();
                GameEvent::FullStateUpdate(state_for_client)
            }
            _ => event_payload,
        };
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &event_to_send,
        ) {
            Ok(wrapped) => {
                self.send_generic_message_to_client_internal(client_id, wrapped)
                    .await
            }
            Err(e) => tracing::error!(
                "DND: Serialize GameEvent err for client {}: {}",
                client_id,
                e
            ),
        }
    }

    async fn broadcast_game_event_to_all_admins(&self, event_payload: GameEvent) {
        let event_to_send = match event_payload {
            GameEvent::FullStateUpdate(mut state_for_client) => {
                state_for_client.prepare_for_client_view();
                GameEvent::FullStateUpdate(state_for_client)
            }
            _ => event_payload,
        };
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &event_to_send,
        ) {
            Ok(wrapped) => {
                self.broadcast_generic_message_to_all_admins_internal(wrapped)
                    .await
            }
            Err(e) => tracing::error!("DND: Serialize GameEvent err for broadcast: {}", e),
        }
    }

    async fn broadcast_full_state_update_internal(&mut self) {
        self.prepare_for_client_view();
        let state_clone_for_event = self.clone();
        // Use broadcast_game_event_to_all_admins to ensure FullStateUpdate is prepared
        self.broadcast_game_event_to_all_admins(GameEvent::FullStateUpdate(state_clone_for_event))
            .await;
    }

    async fn send_generic_message_to_client_internal(
        &self,
        client_id: &Uuid,
        message: GenericServerToClientMessage,
    ) {
        if let Some(tx) = self.clients.get(client_id) {
            if let Ok(ws_msg) = message.to_ws_text() {
                if tx.send(ws_msg).await.is_err() {
                    tracing::warn!("DND: Fail send generic to client {}", client_id);
                }
            } else {
                tracing::error!("DND: Serialize generic err for client {}", client_id);
            }
        }
    }

    async fn broadcast_generic_message_to_all_admins_internal(
        &self,
        message: GenericServerToClientMessage,
    ) {
        if self.clients.is_empty() {
            return;
        }
        if let Ok(ws_msg) = message.to_ws_text() {
            for (id, tx) in &self.clients {
                if tx.send(ws_msg.clone()).await.is_err() {
                    tracing::warn!("DND: Fail broadcast generic to admin {}", id);
                }
            }
        } else {
            tracing::error!("DND: Serialize generic err for broadcast");
        }
    }

    fn initialize_game_board(&mut self) {
        let mut money_shuffled = MONEY_VALUES.to_vec();
        money_shuffled.shuffle(&mut thread_rng());
        self.briefcase_values = money_shuffled;
        self.briefcase_is_opened = vec![false; TOTAL_CASES as usize];
        self.remaining_money_values_in_play = MONEY_VALUES.to_vec();
        self.remaining_money_values_in_play.sort_unstable();
        self.player_chosen_case_index = None;
        self.current_votes_by_user.clear();
        self.current_round_schedule_index = 0;
        self.phase = GamePhase::PlayerCaseSelection_AwaitVoteStart;
        tracing::info!("DND: Game board initialized.");
    }

    fn open_briefcase(&mut self, case_index: usize) -> Option<u64> {
        if case_index < self.briefcase_values.len() && !self.briefcase_is_opened[case_index] {
            self.briefcase_is_opened[case_index] = true;
            let value_opened = self.briefcase_values[case_index];
            if let Some(pos) = self
                .remaining_money_values_in_play
                .iter()
                .position(|&v| v == value_opened)
            {
                self.remaining_money_values_in_play.remove(pos);
            }
            return Some(value_opened);
        }
        None
    }

    fn calculate_banker_offer(&self) -> u64 {
        if self.remaining_money_values_in_play.is_empty() {
            return 0;
        }
        let sum_rem: u64 = self.remaining_money_values_in_play.iter().sum();
        let avg_rem = sum_rem as f64 / self.remaining_money_values_in_play.len() as f64;
        let prog_factor = if ROUND_SCHEDULE.is_empty() {
            0.5
        } else {
            self.current_round_schedule_index as f64 / ROUND_SCHEDULE.len() as f64
        };
        let offer_perc = 0.10 + (prog_factor * 0.75);
        (avg_rem * offer_perc.min(0.85)).round().max(1.0) as u64
    }

    // Renamed to signify it's for internal use and produces the HashMap
    fn tally_current_votes_internal(&self) -> HashMap<String, u32> {
        self.current_votes_by_user
            .values()
            .fold(HashMap::new(), |mut acc, vote_str| {
                *acc.entry(vote_str.clone()).or_insert(0) += 1;
                acc
            })
    }

    fn process_player_case_selection_vote_from_tally(
        &self,
        tally: &HashMap<String, u32>,
    ) -> Option<usize> {
        tally
            .iter()
            .filter_map(|(vote_str, count)| {
                vote_str
                    .parse::<u8>()
                    .ok()
                    .map(|id_1_based| (id_1_based, *count))
            })
            .filter(|(id_1_based, _)| *id_1_based >= 1 && *id_1_based <= TOTAL_CASES)
            .map(|(id_1_based, count)| ((id_1_based - 1) as usize, count))
            .filter(|(idx, _)| {
                *idx < self.briefcase_is_opened.len() && !self.briefcase_is_opened[*idx]
            })
            .max_by_key(|&(_, count)| count)
            .map(|(idx, _)| idx)
    }

    fn process_round_case_opening_votes_from_tally(
        &self,
        tally: &HashMap<String, u32>,
        num_to_select: u8,
    ) -> Vec<usize> {
        let mut sorted: Vec<(usize, u32)> = tally
            .iter()
            .filter_map(|(vote_str, &count)| {
                vote_str
                    .parse::<u8>()
                    .ok()
                    .map(|id_1_based| (id_1_based, count))
            })
            .filter(|(id_1_based, _)| *id_1_based >= 1 && *id_1_based <= TOTAL_CASES)
            .map(|(id_1_based, count)| ((id_1_based - 1) as usize, count))
            .filter(|(idx, _)| {
                *idx < self.briefcase_is_opened.len()
                    && !self.briefcase_is_opened[*idx]
                    && Some(*idx) != self.player_chosen_case_index
            })
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        sorted
            .into_iter()
            .map(|(idx, _)| idx)
            .take(num_to_select as usize)
            .collect()
    }

    fn process_deal_no_deal_votes_from_tally(&self, tally: &HashMap<String, u32>) -> Option<bool> {
        let deal = tally.get("DEAL").cloned().unwrap_or(0);
        let no_deal = tally.get("NO DEAL").cloned().unwrap_or(0);
        if deal == 0 && no_deal == 0 {
            return Some(false);
        }
        Some(deal >= no_deal)
    }

    async fn admin_cmd_start_game(&mut self) {
        if matches!(self.phase, GamePhase::Setup | GamePhase::GameOver { .. }) {
            self.initialize_game_board();
        } else {
            tracing::warn!("DND: StartGame in invalid phase: {:?}", self.phase);
        }
    }

    async fn admin_cmd_start_voting(&mut self) {
        let new_phase_opt = match self.phase {
            GamePhase::PlayerCaseSelection_AwaitVoteStart => {
                Some(GamePhase::PlayerCaseSelection_Voting)
            }
            GamePhase::RoundCaseOpening_AwaitVoteStart {
                round_number,
                total_to_open_for_round,
                opened_so_far_for_round,
            } => {
                if total_to_open_for_round.saturating_sub(opened_so_far_for_round) > 0 {
                    Some(GamePhase::RoundCaseOpening_Voting {
                        round_number,
                        total_to_open_for_round,
                        opened_so_far_for_round,
                    })
                } else {
                    None
                }
            }
            GamePhase::DealOrNoDeal_AwaitVoteStart {
                round_number,
                offer,
            } => Some(GamePhase::DealOrNoDeal_Voting {
                round_number,
                offer,
            }),
            _ => None,
        };

        if let Some(new_phase) = new_phase_opt {
            self.phase = new_phase;
            self.current_votes_by_user.clear();
        } else {
            tracing::warn!(
                "DND: StartVoting in invalid phase or context: {:?}",
                self.phase
            );
        }
    }

    async fn admin_cmd_conclude_voting(&mut self) {
        let current_phase_cloned = self.phase.clone();
        let final_tally = self.tally_current_votes_internal(); // Use internal tally for processing

        match current_phase_cloned {
            GamePhase::PlayerCaseSelection_Voting => {
                if let Some(selected_idx) =
                    self.process_player_case_selection_vote_from_tally(&final_tally)
                {
                    self.player_chosen_case_index = Some(selected_idx);
                    self.current_round_schedule_index = 0;
                    if ROUND_SCHEDULE.is_empty() {
                        self.end_game_no_deal_final_case().await;
                        return;
                    }
                    self.phase = GamePhase::RoundCaseOpening_AwaitVoteStart {
                        round_number: 1,
                        total_to_open_for_round: ROUND_SCHEDULE[0],
                        opened_so_far_for_round: 0,
                    };
                } else {
                    self.phase = GamePhase::PlayerCaseSelection_AwaitVoteStart;
                }
            }
            GamePhase::RoundCaseOpening_Voting {
                round_number,
                total_to_open_for_round,
                mut opened_so_far_for_round,
            } => {
                let needed = total_to_open_for_round.saturating_sub(opened_so_far_for_round);
                if needed > 0 {
                    let indices_to_open =
                        self.process_round_case_opening_votes_from_tally(&final_tally, needed);
                    for idx_to_open in indices_to_open {
                        if let Some(value_opened) = self.open_briefcase(idx_to_open) {
                            opened_so_far_for_round += 1;
                            self.broadcast_game_event_to_all_admins(GameEvent::CaseOpened {
                                case_index: idx_to_open,
                                value: value_opened,
                                is_player_case_reveal_at_end: false,
                            })
                            .await;
                        }
                    }
                }
                if opened_so_far_for_round >= total_to_open_for_round {
                    self.phase = GamePhase::BankerOfferCalculation { round_number };
                    let offer = self.calculate_banker_offer();
                    self.broadcast_game_event_to_all_admins(GameEvent::BankerOfferPresented {
                        offer_amount: offer,
                    })
                    .await;
                    self.phase = GamePhase::DealOrNoDeal_AwaitVoteStart {
                        round_number,
                        offer,
                    };
                } else {
                    self.phase = GamePhase::RoundCaseOpening_AwaitVoteStart {
                        round_number,
                        total_to_open_for_round,
                        opened_so_far_for_round,
                    };
                }
            }
            GamePhase::DealOrNoDeal_Voting {
                round_number,
                offer,
            } => {
                let took_deal = self
                    .process_deal_no_deal_votes_from_tally(&final_tally)
                    .unwrap_or(false);
                let p_case_idx = self
                    .player_chosen_case_index
                    .expect("Player case index missing");
                let p_case_val = self.briefcase_values[p_case_idx];

                if took_deal {
                    let summary = format!(
                        "DEAL! Twitch won ${}. Their case #{} (value ${}) had this amount.",
                        offer,
                        p_case_idx + 1,
                        p_case_val
                    );
                    self.phase = GamePhase::GameOver {
                        summary,
                        winnings: offer,
                        player_case_original_value: p_case_val,
                    };
                } else {
                    self.current_round_schedule_index += 1;
                    let unopened_not_player = self
                        .briefcase_is_opened
                        .iter()
                        .zip(0..)
                        .filter(|(&is_open, idx)| {
                            !is_open && Some(*idx) != self.player_chosen_case_index
                        })
                        .count();

                    if self.current_round_schedule_index >= ROUND_SCHEDULE.len()
                        || unopened_not_player == 0
                    {
                        self.end_game_no_deal_final_case().await;
                        return;
                    } else {
                        let next_r_num = round_number + 1;
                        let sched_open = ROUND_SCHEDULE[self.current_round_schedule_index];
                        let actual_open = std::cmp::min(sched_open, unopened_not_player as u8);
                        if actual_open > 0 {
                            self.phase = GamePhase::RoundCaseOpening_AwaitVoteStart {
                                round_number: next_r_num,
                                total_to_open_for_round: actual_open,
                                opened_so_far_for_round: 0,
                            };
                        } else {
                            self.end_game_no_deal_final_case().await;
                            return;
                        }
                    }
                }
            }
            _ => {
                tracing::warn!("DND: ConcludeVoting in invalid phase: {:?}", self.phase);
                return;
            }
        }
        self.current_votes_by_user.clear();
    }

    async fn end_game_no_deal_final_case(&mut self) {
        let p_case_idx = self
            .player_chosen_case_index
            .expect("Player case index missing");
        if !self.briefcase_is_opened[p_case_idx] {
            self.briefcase_is_opened[p_case_idx] = true;
        }
        let p_case_val = self.briefcase_values[p_case_idx];
        self.broadcast_game_event_to_all_admins(GameEvent::CaseOpened {
            case_index: p_case_idx,
            value: p_case_val,
            is_player_case_reveal_at_end: true,
        })
        .await;
        let summary = format!(
            "NO DEAL! Game ended. Player opened case #{}, winning ${}.",
            p_case_idx + 1,
            p_case_val
        );
        self.phase = GamePhase::GameOver {
            summary,
            winnings: p_case_val,
            player_case_original_value: p_case_val,
        };
    }

    fn validate_and_parse_twitch_vote(
        &self,
        vote_text: &str,
        current_game_phase: &GamePhase,
    ) -> (bool, Option<String>) {
        match current_game_phase {
            GamePhase::PlayerCaseSelection_Voting | GamePhase::RoundCaseOpening_Voting { .. } => {
                if let Ok(case_id_1_based) = vote_text.parse::<u8>() {
                    if (1..=TOTAL_CASES).contains(&case_id_1_based) {
                        let case_idx_0_based = (case_id_1_based - 1) as usize;
                        let is_player_sel =
                            matches!(current_game_phase, GamePhase::PlayerCaseSelection_Voting);
                        if case_idx_0_based < self.briefcase_is_opened.len()
                            && !self.briefcase_is_opened[case_idx_0_based]
                            && (is_player_sel
                                || Some(case_idx_0_based) != self.player_chosen_case_index)
                        {
                            return (true, Some(case_id_1_based.to_string()));
                        }
                    }
                }
            }
            GamePhase::DealOrNoDeal_Voting { .. } => {
                let lower = vote_text.to_lowercase();
                if ["deal", "yes"].contains(&lower.as_str()) {
                    return (true, Some("DEAL".to_string()));
                }
                if ["no", "nodeal", "no deal"].contains(&lower.as_str()) {
                    return (true, Some("NO DEAL".to_string()));
                }
            }
            _ => {}
        }
        (false, None)
    }
}

impl GameLogic for DealNoDealGame {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        self.clients.insert(client_id.clone(), client_tx);
        self.prepare_for_client_view();
        let state_clone = self.clone();
        // Send FullStateUpdate with the cloned, prepared state to the newly connected client
        if let Ok(wrapped_message) = GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &GameEvent::FullStateUpdate(state_clone), // Send prepared clone
        ) {
            self.send_generic_message_to_client_internal(&client_id, wrapped_message)
                .await;
        } else {
            tracing::error!(
                "DND: Failed to serialize FullStateUpdate for new client {}",
                client_id
            );
        }
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        self.clients.remove(&client_id);
    }

    async fn handle_event(&mut self, _client_id: Uuid, message: GenericClientToServerMessage) {
        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
            } => {
                if game_type_id != self.game_type_id() {
                    /* error */
                    return;
                }
                match serde_json::from_value::<AdminCommand>(command_data) {
                    Ok(cmd) => {
                        match cmd {
                            AdminCommand::StartGame => self.admin_cmd_start_game().await,
                            AdminCommand::StartVoting => self.admin_cmd_start_voting().await,
                            AdminCommand::ConcludeVotingAndProcess => {
                                self.admin_cmd_conclude_voting().await
                            }
                        }
                        self.broadcast_full_state_update_internal().await;
                    }
                    Err(e) => {
                        tracing::error!("DND: Deserialize AdminCommand err: {}", e);
                        self.broadcast_full_state_update_internal().await;
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { .. } => {}
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        let is_voting_active_phase = matches!(
            self.phase,
            GamePhase::PlayerCaseSelection_Voting
                | GamePhase::RoundCaseOpening_Voting { .. }
                | GamePhase::DealOrNoDeal_Voting { .. }
        );
        if !is_voting_active_phase {
            return;
        }

        let (is_valid, parsed_vote_value_opt) =
            self.validate_and_parse_twitch_vote(message.text.trim(), &self.phase);

        if is_valid {
            if let Some(vote_value_str) = parsed_vote_value_opt {
                let voter_username = message.sender_username;
                // Update internal tracking of votes
                self.current_votes_by_user
                    .insert(voter_username.clone(), vote_value_str.clone());

                // Broadcast the lightweight PlayerVoteRegistered event
                self.broadcast_game_event_to_all_admins(GameEvent::PlayerVoteRegistered {
                    voter_username,
                    vote_value: vote_value_str,
                })
                .await;
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }
    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_DND.to_string()
    }
    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }
}
