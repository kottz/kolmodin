// src/game_logic/deal_no_deal.rs
use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::game_logic::GameLogic;
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::twitch_integration::ParsedTwitchMessage;

const GAME_TYPE_ID_DND: &str = "DealNoDeal";

const TOTAL_CASES: u8 = 26;
const MONEY_VALUES: [u64; TOTAL_CASES as usize] = [
    1, 3, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1_000, 5_000, 10_000, 25_000, 50_000,
    75_000, 100_000, 200_000, 300_000, 400_000, 500_000, 750_000, 1_000_000,
];
const ROUND_SCHEDULE: [u8; 6] = [6, 5, 4, 3, 2, 1];

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
    // StartVoting, // Removed: Voting starts automatically
    ConcludeVotingAndProcess,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    FullStateUpdate(DealNoDealGame),
    PlayerVoteRegistered {
        voter_username: String,
        vote_value: String,
    },
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
#[serde(tag = "type", content = "data")]
pub enum GamePhase {
    Setup,
    PlayerCaseSelection_Voting,
    RoundCaseOpening_Voting {
        round_number: u8,
        total_to_open_for_round: u8,
        opened_so_far_for_round: u8,
    },
    BankerOfferCalculation {
        // This is a brief, intermediate phase
        round_number: u8,
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
    pub current_round_schedule_index: usize, // 0-based index for ROUND_SCHEDULE
    current_votes_by_user: HashMap<String, String>,

    // Derived fields, populated by prepare_for_client_view
    pub current_round_display_number: u8, // 1-based for display
    pub cases_to_open_this_round_target: u8,
    pub cases_opened_in_current_round_segment: u8,
    pub banker_offer: Option<u64>,
    pub current_vote_tally: Option<HashMap<String, u32>>,
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
            GamePhase::Setup => (0, 0, 0, None, None),
            GamePhase::PlayerCaseSelection_Voting => {
                (0, 0, 0, Some(self.tally_current_votes_internal()), None)
            }
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
            GamePhase::DealOrNoDeal_Voting {
                round_number, // This is the 1-based display round number for the offer
                offer: current_offer,
            } => {
                // self.current_round_schedule_index refers to the 0-based index of the completed round in ROUND_SCHEDULE
                // E.g., after first round (index 0 of schedule) completes, current_round_schedule_index is 0.
                // Offer is for "end of round 1", so round_number (display) is 1.
                let schedule_idx_of_completed_round = self.current_round_schedule_index;

                let completed_round_target =
                    if schedule_idx_of_completed_round < ROUND_SCHEDULE.len() {
                        ROUND_SCHEDULE[schedule_idx_of_completed_round]
                    } else {
                        // This case implies an offer after all scheduled rounds, or if ROUND_SCHEDULE is empty.
                        // For DND, an offer usually follows a round with cases opened.
                        // If ROUND_SCHEDULE[0] was 0, current_round_schedule_index is 0, ROUND_SCHEDULE[0] is 0.
                        0
                    };

                (
                    *round_number,          // Display round number of the offer
                    completed_round_target, // Target for the round that just finished
                    completed_round_target, // All cases for that round are considered "opened" for this view
                    Some(self.tally_current_votes_internal()),
                    Some(*current_offer),
                )
            }
            GamePhase::GameOver { winnings, .. } => {
                (ROUND_SCHEDULE.len() as u8 + 1, 0, 0, None, Some(*winnings)) // Display a "final" round number
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
        self.current_round_schedule_index = 0; // Reset round schedule index

        self.phase = GamePhase::PlayerCaseSelection_Voting;
        self.current_votes_by_user.clear();
        tracing::info!("DND: Game board initialized. Phase: PlayerCaseSelection_Voting.");
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

        // current_round_schedule_index is 0-based for the round just completed or for which offer is being made.
        // For progression, if index 0 completed, that's 1 round done.
        let rounds_completed_for_progression = self.current_round_schedule_index + 1;

        let prog_factor = if ROUND_SCHEDULE.is_empty() {
            0.5
        } else {
            rounds_completed_for_progression as f64 / ROUND_SCHEDULE.len() as f64
        };

        let offer_perc = 0.10 + (prog_factor * 0.75);
        (avg_rem * offer_perc.min(0.85)).round().max(1.0) as u64
    }

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
            return Some(false); // Default to NO DEAL if no votes
        }
        Some(deal >= no_deal)
    }

    async fn admin_cmd_start_game(&mut self) {
        if matches!(self.phase, GamePhase::Setup | GamePhase::GameOver { .. }) {
            self.initialize_game_board();
        } else {
            tracing::warn!("DND: StartGame called in invalid phase: {:?}", self.phase);
        }
    }

    async fn admin_cmd_conclude_voting(&mut self) {
        let current_phase_cloned = self.phase.clone();
        let final_tally = self.tally_current_votes_internal();

        match current_phase_cloned {
            GamePhase::PlayerCaseSelection_Voting => {
                if let Some(selected_idx) =
                    self.process_player_case_selection_vote_from_tally(&final_tally)
                {
                    self.player_chosen_case_index = Some(selected_idx);
                    self.current_round_schedule_index = 0; // current_round_schedule_index is 0-based for ROUND_SCHEDULE

                    if ROUND_SCHEDULE.is_empty() {
                        self.end_game_no_deal_final_case().await;
                        return;
                    }

                    let cases_to_open_first_round = ROUND_SCHEDULE[0];
                    if cases_to_open_first_round == 0 {
                        // Special case: immediate offer if first round opens 0 cases
                        self.phase = GamePhase::BankerOfferCalculation { round_number: 1 }; // Display as Round 1 offer
                        let offer = self.calculate_banker_offer(); // Uses current_round_schedule_index = 0
                        self.broadcast_game_event_to_all_admins(GameEvent::BankerOfferPresented {
                            offer_amount: offer,
                        })
                        .await;
                        self.phase = GamePhase::DealOrNoDeal_Voting {
                            round_number: 1, // Offer is for "Round 1" (after 0 cases opened)
                            offer,
                        };
                        self.current_votes_by_user.clear();
                    } else {
                        // Normal first round
                        self.phase = GamePhase::RoundCaseOpening_Voting {
                            round_number: 1, // 1-based display number
                            total_to_open_for_round: cases_to_open_first_round,
                            opened_so_far_for_round: 0,
                        };
                        self.current_votes_by_user.clear();
                    }
                } else {
                    // No valid case selected, remain in PlayerCaseSelection_Voting. Clear votes for a fresh attempt.
                    self.current_votes_by_user.clear();
                    tracing::warn!(
                        "DND: No valid player case selected. Awaiting more votes or re-concluding."
                    );
                }
            }
            GamePhase::RoundCaseOpening_Voting {
                round_number, // This is 1-based display round number
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
                        if opened_so_far_for_round >= total_to_open_for_round {
                            break;
                        }
                    }
                }

                if opened_so_far_for_round >= total_to_open_for_round {
                    // current_round_schedule_index should be correct here, pointing to the round just finished.
                    self.phase = GamePhase::BankerOfferCalculation { round_number };
                    let offer = self.calculate_banker_offer();
                    self.broadcast_game_event_to_all_admins(GameEvent::BankerOfferPresented {
                        offer_amount: offer,
                    })
                    .await;
                    self.phase = GamePhase::DealOrNoDeal_Voting {
                        round_number, // Offer is for the round that just finished
                        offer,
                    };
                    self.current_votes_by_user.clear();
                } else {
                    // Not enough cases opened, stay in voting for this round segment
                    self.phase = GamePhase::RoundCaseOpening_Voting {
                        round_number,
                        total_to_open_for_round,
                        opened_so_far_for_round, // Use updated local copy
                    };
                    self.current_votes_by_user.clear(); // Clear votes for fresh voting for remaining cases
                    tracing::warn!(
                        "DND: Not enough cases opened for round {}. Target: {}, Opened: {}. Awaiting more votes.",
                        round_number,
                        total_to_open_for_round,
                        opened_so_far_for_round
                    );
                }
            }
            GamePhase::DealOrNoDeal_Voting {
                round_number, // 1-based display round number of the offer
                offer,
            } => {
                let took_deal = self
                    .process_deal_no_deal_votes_from_tally(&final_tally)
                    .unwrap_or(false);

                let p_case_idx = self
                    .player_chosen_case_index
                    .expect("Player case index missing during DealOrNoDeal");
                let p_case_val = self.briefcase_values[p_case_idx];

                if took_deal {
                    let summary = format!(
                        "DEAL! Twitch won ${}. Their chosen case #{} contained ${}.",
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
                    // NO DEAL
                    // current_round_schedule_index was for the round just completed. Advance it for the next round.
                    self.current_round_schedule_index += 1;

                    let unopened_not_player = self
                        .briefcase_is_opened
                        .iter()
                        .zip(0..)
                        .filter(|&(is_open, ref idx)| {
                            !is_open && Some(*idx) != self.player_chosen_case_index
                        })
                        .count();

                    if self.current_round_schedule_index >= ROUND_SCHEDULE.len()
                        || unopened_not_player == 0
                    {
                        self.end_game_no_deal_final_case().await;
                        return;
                    } else {
                        let next_display_r_num = round_number + 1;
                        let sched_open_for_next_round =
                            ROUND_SCHEDULE[self.current_round_schedule_index];
                        let actual_open_for_next_round =
                            std::cmp::min(sched_open_for_next_round, unopened_not_player as u8);

                        if actual_open_for_next_round > 0 {
                            self.phase = GamePhase::RoundCaseOpening_Voting {
                                round_number: next_display_r_num,
                                total_to_open_for_round: actual_open_for_next_round,
                                opened_so_far_for_round: 0,
                            };
                            self.current_votes_by_user.clear();
                        } else {
                            self.end_game_no_deal_final_case().await;
                            return;
                        }
                    }
                }
            }
            _ => {
                tracing::warn!(
                    "DND: ConcludeVotingAndProcess called in invalid phase: {:?}",
                    self.phase
                );
                return;
            }
        }
    }

    async fn end_game_no_deal_final_case(&mut self) {
        let p_case_idx = self
            .player_chosen_case_index
            .expect("Player case index missing at end_game_no_deal_final_case");

        let p_case_val = self.briefcase_values[p_case_idx];
        if !self.briefcase_is_opened[p_case_idx] {
            self.open_briefcase(p_case_idx); // Mark as opened and remove from remaining values
        }

        self.broadcast_game_event_to_all_admins(GameEvent::CaseOpened {
            case_index: p_case_idx,
            value: p_case_val,
            is_player_case_reveal_at_end: true,
        })
        .await;

        let summary = format!(
            "NO DEAL! The game concluded. Player's chosen case #{} contained ${}.",
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
                        let is_player_sel_phase =
                            matches!(current_game_phase, GamePhase::PlayerCaseSelection_Voting);

                        if case_idx_0_based < self.briefcase_is_opened.len()
                            && !self.briefcase_is_opened[case_idx_0_based]
                        {
                            if is_player_sel_phase
                                || Some(case_idx_0_based) != self.player_chosen_case_index
                            {
                                return (true, Some(case_id_1_based.to_string()));
                            }
                        }
                    }
                }
            }
            GamePhase::DealOrNoDeal_Voting { .. } => {
                let lower = vote_text.to_lowercase();
                if ["deal", "yes", "d"].contains(&lower.as_str()) {
                    return (true, Some("DEAL".to_string()));
                }
                if ["no", "nodeal", "no deal", "n"].contains(&lower.as_str()) {
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
        if let Ok(wrapped_message) = GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &GameEvent::FullStateUpdate(state_clone),
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
                    tracing::warn!(
                        "DND: Received command for wrong game_type_id: {}",
                        game_type_id
                    );
                    return;
                }
                match serde_json::from_value::<AdminCommand>(command_data) {
                    Ok(cmd) => {
                        match cmd {
                            AdminCommand::StartGame => self.admin_cmd_start_game().await,
                            AdminCommand::ConcludeVotingAndProcess => {
                                self.admin_cmd_conclude_voting().await
                            }
                        }
                        self.broadcast_full_state_update_internal().await;
                    }
                    Err(e) => {
                        tracing::error!("DND: Deserialize AdminCommand err: {}", e);
                        self.broadcast_full_state_update_internal().await; // Send current state back
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { .. } => {
                tracing::trace!("DND: Received GlobalCommand (unhandled by DND specific logic)");
            }
            _ => {
                tracing::warn!("DND: Received unrecognized message type");
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        let current_phase_clone = self.phase.clone();
        let is_voting_active_phase = matches!(
            current_phase_clone,
            GamePhase::PlayerCaseSelection_Voting
                | GamePhase::RoundCaseOpening_Voting { .. }
                | GamePhase::DealOrNoDeal_Voting { .. }
        );

        if !is_voting_active_phase {
            return;
        }

        let (is_valid, parsed_vote_value_opt) =
            self.validate_and_parse_twitch_vote(message.text.trim(), &current_phase_clone);

        if is_valid {
            if let Some(vote_value_str) = parsed_vote_value_opt {
                let voter_username = message.sender_username;

                self.current_votes_by_user
                    .insert(voter_username.clone(), vote_value_str.clone());

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

    fn get_all_client_ids(&self) -> Vec<Uuid> {
        self.clients.keys().copied().collect()
    }
}
