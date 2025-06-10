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
use crate::twitch::ParsedTwitchMessage;

const GAME_TYPE_ID_DND: &str = "DealNoDeal";

const TOTAL_CASES: u8 = 26;
const MONEY_VALUES: [u64; TOTAL_CASES as usize] = [
    1, 3, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1_000, 5_000, 10_000, 25_000, 50_000,
    75_000, 100_000, 200_000, 300_000, 400_000, 500_000, 750_000, 1_000_000,
];

const ROUND_SCHEDULE: [u8; 9] = [6, 5, 4, 3, 2, 1, 1, 1, 1];

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")]
pub enum AdminCommand {
    StartGame,
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
    PlayerCaseSelectionVoting,
    RoundCaseOpeningVoting {
        round_number: u8,
        total_to_open_for_round: u8,
        opened_so_far_for_round: u8,
    },
    BankerOfferCalculation {
        round_number: u8,
    },
    DealOrNoDealVoting {
        round_number: u8,
        offer: u64,
    },
    SwitchOrKeepVoting {
        final_case_index: usize,
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
    current_votes_by_user: HashMap<String, String>,

    pub current_round_display_number: u8,
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
            GamePhase::PlayerCaseSelectionVoting => {
                (0, 0, 0, Some(self.tally_current_votes_internal()), None)
            }
            GamePhase::RoundCaseOpeningVoting {
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
            GamePhase::DealOrNoDealVoting {
                round_number,
                offer: current_offer,
            } => {
                let schedule_idx_of_completed_round = self.current_round_schedule_index;

                let completed_round_target =
                    if schedule_idx_of_completed_round < ROUND_SCHEDULE.len() {
                        ROUND_SCHEDULE[schedule_idx_of_completed_round]
                    } else {
                        0
                    };

                (
                    *round_number,
                    completed_round_target,
                    completed_round_target,
                    Some(self.tally_current_votes_internal()),
                    Some(*current_offer),
                )
            }
            GamePhase::SwitchOrKeepVoting { .. } => (
                ROUND_SCHEDULE.len() as u8 + 1,
                0,
                0,
                Some(self.tally_current_votes_internal()),
                None,
            ),
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
                client.id = %client_id,
                error = %e,
                "Failed to serialize GameEvent for client"
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
            Err(e) => tracing::error!(
                error = %e,
                "Failed to serialize GameEvent for broadcast"
            ),
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
                    tracing::warn!(
                        client.id = %client_id,
                        "Failed to send generic message to client"
                    );
                }
            } else {
                tracing::error!(
                    client.id = %client_id,
                    "Failed to serialize generic message for client"
                );
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
                    tracing::warn!(
                        client.id = %id,
                        "Failed to broadcast generic message to admin"
                    );
                }
            }
        } else {
            tracing::error!("Failed to serialize generic message for broadcast");
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
        self.current_round_schedule_index = 0;

        self.phase = GamePhase::PlayerCaseSelectionVoting;
        self.current_votes_by_user.clear();
        tracing::info!(
            phase = "PlayerCaseSelectionVoting",
            "Game board initialized"
        );
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

        let rounds_completed_for_progression = self.current_round_schedule_index + 1;

        let prog_factor = rounds_completed_for_progression as f64 / ROUND_SCHEDULE.len() as f64;

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
            return Some(false);
        }
        Some(deal >= no_deal)
    }

    fn process_switch_or_keep_votes_from_tally(
        &self,
        tally: &HashMap<String, u32>,
    ) -> Option<bool> {
        let switch_votes = tally.get("SWITCH").cloned().unwrap_or(0);
        let keep_votes = tally.get("KEEP").cloned().unwrap_or(0);
        if switch_votes == 0 && keep_votes == 0 {
            return Some(false);
        }
        Some(switch_votes > keep_votes)
    }

    async fn admin_cmd_start_game(&mut self) {
        if matches!(self.phase, GamePhase::Setup | GamePhase::GameOver { .. }) {
            self.initialize_game_board();
        } else {
            tracing::warn!(
                phase = ?self.phase,
                "StartGame called in invalid phase"
            );
        }
    }

    async fn admin_cmd_conclude_voting(&mut self) {
        let current_phase_cloned = self.phase.clone();
        let final_tally = self.tally_current_votes_internal();

        match current_phase_cloned {
            GamePhase::PlayerCaseSelectionVoting => {
                if let Some(selected_idx) =
                    self.process_player_case_selection_vote_from_tally(&final_tally)
                {
                    self.player_chosen_case_index = Some(selected_idx);
                    self.current_round_schedule_index = 0;

                    let cases_to_open_first_round = ROUND_SCHEDULE[0];
                    if cases_to_open_first_round == 0 {
                        self.phase = GamePhase::BankerOfferCalculation { round_number: 1 };
                        let offer = self.calculate_banker_offer();
                        self.broadcast_game_event_to_all_admins(GameEvent::BankerOfferPresented {
                            offer_amount: offer,
                        })
                        .await;
                        self.phase = GamePhase::DealOrNoDealVoting {
                            round_number: 1,
                            offer,
                        };
                        self.current_votes_by_user.clear();
                    } else {
                        self.phase = GamePhase::RoundCaseOpeningVoting {
                            round_number: 1,
                            total_to_open_for_round: cases_to_open_first_round,
                            opened_so_far_for_round: 0,
                        };
                        self.current_votes_by_user.clear();
                    }
                } else {
                    self.current_votes_by_user.clear();
                    tracing::warn!(
                        "No valid player case selected. Awaiting more votes or re-concluding"
                    );
                }
            }
            GamePhase::RoundCaseOpeningVoting {
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
                        if opened_so_far_for_round >= total_to_open_for_round {
                            break;
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
                    self.phase = GamePhase::DealOrNoDealVoting {
                        round_number,
                        offer,
                    };
                    self.current_votes_by_user.clear();
                } else {
                    self.phase = GamePhase::RoundCaseOpeningVoting {
                        round_number,
                        total_to_open_for_round,
                        opened_so_far_for_round,
                    };
                    self.current_votes_by_user.clear();
                    tracing::warn!(
                        round.number = round_number,
                        cases.target = total_to_open_for_round,
                        cases.opened = opened_so_far_for_round,
                        "Not enough cases opened for round. Awaiting more votes"
                    );
                }
            }
            GamePhase::DealOrNoDealVoting {
                round_number,
                offer,
            } => {
                let took_deal = self
                    .process_deal_no_deal_votes_from_tally(&final_tally)
                    .unwrap_or(false);

                let p_case_idx = match self.player_chosen_case_index {
                    Some(idx) => idx,
                    None => {
                        tracing::error!(
                            "Player case index missing during DealOrNoDeal - defaulting to case 0"
                        );
                        0
                    }
                };
                let p_case_val = self
                    .briefcase_values
                    .get(p_case_idx)
                    .copied()
                    .unwrap_or_else(|| {
                        tracing::error!(
                            case.index = p_case_idx,
                            "Invalid case index during DealOrNoDeal - using default value"
                        );
                        1000000
                    });

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
                    self.current_round_schedule_index += 1;

                    let unopened_not_player: Vec<usize> = self
                        .briefcase_is_opened
                        .iter()
                        .zip(0..)
                        .filter(|&(is_open, idx)| {
                            !is_open && Some(idx) != self.player_chosen_case_index
                        })
                        .map(|(_, idx)| idx)
                        .collect();

                    if unopened_not_player.len() == 1 {
                        // Final decision: Switch or Keep
                        let final_case_idx = unopened_not_player[0];
                        self.phase = GamePhase::SwitchOrKeepVoting {
                            final_case_index: final_case_idx,
                        };
                        self.current_votes_by_user.clear();
                    } else if self.current_round_schedule_index >= ROUND_SCHEDULE.len() {
                        self.end_game_no_deal_final_case().await;
                        return;
                    } else {
                        let next_display_r_num = round_number + 1;
                        let cases_to_open_next_round = ROUND_SCHEDULE
                            .get(self.current_round_schedule_index)
                            .copied()
                            .unwrap_or_else(|| {
                                tracing::error!(
                                    schedule.index = self.current_round_schedule_index,
                                    "Invalid round schedule index - using default value"
                                );
                                1
                            });
                        let actual_open_for_next_round = std::cmp::min(
                            cases_to_open_next_round,
                            unopened_not_player.len() as u8,
                        );

                        if actual_open_for_next_round > 0 {
                            self.phase = GamePhase::RoundCaseOpeningVoting {
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
            GamePhase::SwitchOrKeepVoting { final_case_index } => {
                let should_switch = self
                    .process_switch_or_keep_votes_from_tally(&final_tally)
                    .unwrap_or(false);

                let p_case_idx = match self.player_chosen_case_index {
                    Some(idx) => idx,
                    None => {
                        tracing::error!(
                            "Player case index missing during SwitchOrKeep - defaulting to case 0"
                        );
                        0
                    }
                };
                let p_case_val = self
                    .briefcase_values
                    .get(p_case_idx)
                    .copied()
                    .unwrap_or_else(|| {
                        tracing::error!(
                            case.index = p_case_idx,
                            "Invalid case index during SwitchOrKeep - using default value"
                        );
                        1000000
                    });
                let final_case_val = self
                    .briefcase_values
                    .get(final_case_index)
                    .copied()
                    .unwrap_or_else(|| {
                        tracing::error!(
                            final_case.index = final_case_index,
                            "Invalid final case index during SwitchOrKeep - using default value"
                        );
                        1000000
                    });

                if final_case_index < self.briefcase_is_opened.len()
                    && !self.briefcase_is_opened[final_case_index]
                {
                    self.open_briefcase(final_case_index);
                }

                self.broadcast_game_event_to_all_admins(GameEvent::CaseOpened {
                    case_index: final_case_index,
                    value: final_case_val,
                    is_player_case_reveal_at_end: false,
                })
                .await;

                let (winnings, summary) = if should_switch {
                    (
                        final_case_val,
                        format!(
                            "SWITCH! Twitch switched and won ${}! Their original case #{} contained ${}.",
                            final_case_val,
                            p_case_idx + 1,
                            p_case_val
                        ),
                    )
                } else {
                    if !self.briefcase_is_opened[p_case_idx] {
                        self.open_briefcase(p_case_idx);
                    }

                    self.broadcast_game_event_to_all_admins(GameEvent::CaseOpened {
                        case_index: p_case_idx,
                        value: p_case_val,
                        is_player_case_reveal_at_end: true,
                    })
                    .await;

                    (
                        p_case_val,
                        format!(
                            "KEEP! Twitch kept their original case #{} and won ${}! The other case contained ${}.",
                            p_case_idx + 1,
                            p_case_val,
                            final_case_val
                        ),
                    )
                };

                self.phase = GamePhase::GameOver {
                    summary,
                    winnings,
                    player_case_original_value: p_case_val,
                };
            }
            _ => {
                tracing::warn!(
                    phase = ?self.phase,
                    "ConcludeVotingAndProcess called in invalid phase"
                );
            }
        }
    }

    async fn end_game_no_deal_final_case(&mut self) {
        let p_case_idx = match self.player_chosen_case_index {
            Some(idx) => idx,
            None => {
                tracing::error!(
                    "Player case index missing at end_game_no_deal_final_case - defaulting to case 0"
                );
                0
            }
        };

        let p_case_val = self
            .briefcase_values
            .get(p_case_idx)
            .copied()
            .unwrap_or_else(|| {
                tracing::error!(
                    case.index = p_case_idx,
                    "Invalid case index at end_game_no_deal_final_case - using default value"
                );
                1000000
            });

        if p_case_idx < self.briefcase_is_opened.len() && !self.briefcase_is_opened[p_case_idx] {
            self.open_briefcase(p_case_idx);
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
            GamePhase::PlayerCaseSelectionVoting | GamePhase::RoundCaseOpeningVoting { .. } => {
                if let Ok(case_id_1_based) = vote_text.parse::<u8>() {
                    if (1..=TOTAL_CASES).contains(&case_id_1_based) {
                        let case_idx_0_based = (case_id_1_based - 1) as usize;
                        let is_player_sel_phase =
                            matches!(current_game_phase, GamePhase::PlayerCaseSelectionVoting);

                        if case_idx_0_based < self.briefcase_is_opened.len()
                            && !self.briefcase_is_opened[case_idx_0_based]
                            && (is_player_sel_phase
                                || Some(case_idx_0_based) != self.player_chosen_case_index)
                        {
                            return (true, Some(case_id_1_based.to_string()));
                        }
                    }
                }
            }
            GamePhase::DealOrNoDealVoting { .. } => {
                let lower = vote_text.to_lowercase();
                if ["deal", "yes", "d"].contains(&lower.as_str()) {
                    return (true, Some("DEAL".to_string()));
                }
                if ["no", "nodeal", "no deal", "n"].contains(&lower.as_str()) {
                    return (true, Some("NO DEAL".to_string()));
                }
            }
            GamePhase::SwitchOrKeepVoting { .. } => {
                let lower = vote_text.to_lowercase();
                if ["switch", "swap", "yes", "s"].contains(&lower.as_str()) {
                    return (true, Some("SWITCH".to_string()));
                }
                if ["keep", "stay", "no", "k"].contains(&lower.as_str()) {
                    return (true, Some("KEEP".to_string()));
                }
            }
            _ => {}
        }
        (false, None)
    }
}

impl GameLogic for DealNoDealGame {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        self.clients.insert(client_id, client_tx);
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
                client.id = %client_id,
                "Failed to serialize FullStateUpdate for new client"
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
                        game.type_id = %game_type_id,
                        "Received command for wrong game_type_id"
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
                        tracing::error!(
                            error = %e,
                            "Failed to deserialize AdminCommand"
                        );
                        self.broadcast_full_state_update_internal().await;
                    }
                }
            }
            GenericClientToServerMessage::LeaveLobby => {
                tracing::info!(
                    client.id = %_client_id,
                    "Client explicitly leaving lobby"
                );
                self.client_disconnected(_client_id).await;
            }
            GenericClientToServerMessage::GlobalCommand { .. } => {
                tracing::trace!("Received GlobalCommand (unhandled by DND specific logic)");
            }
            _ => {
                tracing::warn!("Received unrecognized message type");
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        let current_phase_clone = self.phase.clone();
        let is_voting_active_phase = matches!(
            current_phase_clone,
            GamePhase::PlayerCaseSelectionVoting
                | GamePhase::RoundCaseOpeningVoting { .. }
                | GamePhase::DealOrNoDealVoting { .. }
                | GamePhase::SwitchOrKeepVoting { .. }
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
