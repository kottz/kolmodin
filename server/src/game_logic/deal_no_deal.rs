// src/game_logic/deal_or_no_deal_game/mod.rs
use axum::extract::ws;
use rand::seq::SliceRandom;
use rand::thread_rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

// Import generic message types from the main messages module
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};

use crate::game_logic::GameLogic;
use crate::twitch_integration::ParsedTwitchMessage;

const GAME_TYPE_ID_DND: &str = "DealNoDeal";

// Constants for DND game rules
const ACTUAL_TOTAL_CASES: u8 = 22; // Or 26
const ACTUAL_MONEY_VALUES: [u64; ACTUAL_TOTAL_CASES as usize] = [
    1, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1_000, 5_000, 10_000, 25_000, 50_000,
    75_000, 100_000, 250_000, 500_000, 1_000_000,
];
// Defines how many cases to open in each round. Sum should be ACTUAL_TOTAL_CASES - 1
const ROUND_SCHEDULE: [u8; 6] = [6, 5, 4, 3, 2, 1]; // Total 21 cases opened in rounds

// --- Deal Or No Deal Admin Commands (Client -> Server, specific to DND) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "command")] // Using "command" as the tag for DND admin commands
pub enum DNDAdminCommand {
    StartGame,
    StartPlayerCaseSelectionVote,
    StartRoundCaseOpeningVote,
    StartDealNoDealVote,
    ConcludeVotingAndProcess,
    // Example for future: ForceOpenCase { case_id: u8 },
}

// --- Deal Or No Deal Game Events (Server -> Client, specific to DND) ---
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "event_type", content = "data")] // "event_type" for DND specific events
pub enum DNDGameEvent {
    GameStateUpdate(DNDFullGameState),
    TwitchVoteReceived {
        voter_twitch_username: String,
        raw_vote_text: String,
        is_valid_vote: bool,
        parsed_vote_value: Option<String>, // e.g., "15" for case, "DEAL" for decision
        vote_context: DNDVoteTypeContext,
    },
    VotingPeriodChange {
        is_active: bool,
        vote_context: DNDVoteTypeContext,
        instruction_or_outcome: String,
        final_tally: Option<HashMap<String, u32>>, // Vote_option -> Count
    },
    CaseOpened {
        case_id: u8,
        value: u64,
        is_player_case_reveal_at_end: bool,
    },
    BankerOffer {
        offer_amount: u64,
    },
    GameEnded {
        summary: String,
        winnings: u64,
        player_case_original_value: u64,
    },
    // You could also have game-specific errors
    // DNDError { message: String },
}

// --- Supporting Data Structures for DND ---
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DNDBriefcaseClientInfo {
    pub id: u8,
    pub is_opened: bool,
    pub value: Option<u64>, // Only revealed if opened
    pub is_player_case: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DNDFullGameState {
    pub internal_state_tag: String, // e.g., "PlayerCaseSelectionVoting"
    pub current_phase_description: String,
    pub briefcases: Vec<DNDBriefcaseClientInfo>,
    pub player_chosen_case_id: Option<u8>,
    pub remaining_money_values: Vec<u64>,
    pub current_round: u8,
    pub cases_to_open_this_round: u8,
    pub cases_opened_in_current_round: u8,
    pub banker_offer: Option<u64>,
    pub current_vote_tally: Option<HashMap<String, u32>>,
    pub admin_instructions: String, // What admins can/should do next via DNDAdminCommand
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum DNDVoteTypeContext {
    SelectPlayerCase,
    OpenRoundCases { num_expected: u8 },
    DealOrNoDeal,
}

// Internal briefcase structure (not directly sent to client unless part of DNDBriefcaseClientInfo)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Briefcase {
    // pub(super) to keep it within the deal_no_deal_game module
    pub(super) id: u8,
    pub(super) value: u64,
    pub(super) is_opened: bool,
    pub(super) is_player_case: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DNDInternalState {
    NotStarted,
    GameInitializing,
    ReadyForPlayerCaseSelection,
    PlayerCaseSelectionVoting,
    PlayerCaseSelectedMovingToRound {
        round_num: u8,
    },
    ReadyForRoundStart {
        round_num: u8,
        num_to_open_in_round: u8,
    },
    RoundCaseOpeningVoting {
        round_num: u8,
        num_to_open_in_round: u8,
        cases_chosen_for_opening_count: u8,
    },
    CasesOpenedMovingToOffer {
        round_num: u8,
    },
    BankerOfferPresented {
        round_num: u8,
        offer: u64,
    },
    DealNoDealVoting {
        round_num: u8,
        offer: u64,
    },
    GameEndedDeal {
        winnings: u64,
        player_case_value: u64,
    },
    GameEndedNoDeal {
        winnings: u64,
    },
}

#[derive(Debug)]
pub struct DealNoDealGame {
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>, // Admin clients
    internal_state: DNDInternalState,
    briefcases: Vec<Briefcase>,
    player_chosen_case_id: Option<u8>,
    is_voting_active: bool,
    current_vote_context: Option<DNDVoteTypeContext>,
    current_votes_by_user: HashMap<String, String>, // TwitchUsername -> VoteString (e.g., "12", "DEAL")
    current_round_index: usize,                     // Index into ROUND_SCHEDULE
    remaining_money_values_in_play: Vec<u64>,       // Values in UNOPENED cases
    last_banker_offer: Option<u64>,
}

impl DealNoDealGame {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
            internal_state: DNDInternalState::NotStarted,
            briefcases: Vec::new(),
            player_chosen_case_id: None,
            is_voting_active: false,
            current_vote_context: None,
            current_votes_by_user: HashMap::new(),
            current_round_index: 0,
            remaining_money_values_in_play: Vec::new(),
            last_banker_offer: None,
        }
    }

    // --- Helper methods for sending DND-specific events wrapped in GenericServerToClientMessage ---
    async fn send_dnd_event_to_client(&self, client_id: &Uuid, event_payload: DNDGameEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.send_generic_message_to_client(client_id, wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!(
                    "DND: Failed to serialize DNDGameEvent for client {}: {}",
                    client_id,
                    e
                );
            }
        }
    }

    async fn broadcast_dnd_event_to_all_admins(&self, event_payload: DNDGameEvent) {
        match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_DND.to_string(),
            &event_payload,
        ) {
            Ok(wrapped_message) => {
                self.broadcast_generic_message_to_all_admins(wrapped_message)
                    .await;
            }
            Err(e) => {
                tracing::error!("DND: Failed to serialize DNDGameEvent for broadcast: {}", e);
            }
        }
    }

    // Renamed from broadcast_game_state_to_admins for clarity
    async fn broadcast_current_full_game_state(&self) {
        let state_payload = self.construct_full_game_state_payload();
        self.broadcast_dnd_event_to_all_admins(DNDGameEvent::GameStateUpdate(state_payload))
            .await;
    }

    // --- Underlying generic sender methods (could be in a shared utility or base struct later) ---
    async fn send_generic_message_to_client(
        &self,
        client_id: &Uuid,
        message: GenericServerToClientMessage,
    ) {
        if let Some(tx) = self.clients.get(client_id) {
            match message.to_ws_text() {
                Ok(ws_msg) => {
                    if tx.send(ws_msg).await.is_err() {
                        tracing::warn!(
                            "DND: Failed to send generic message to client {}",
                            client_id
                        );
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "DND: Failed to serialize generic message for client {}: {}",
                        client_id,
                        e
                    );
                }
            }
        }
    }

    async fn broadcast_generic_message_to_all_admins(&self, message: GenericServerToClientMessage) {
        if self.clients.is_empty() {
            return;
        }
        match message.to_ws_text() {
            Ok(ws_msg) => {
                for (id, tx) in &self.clients {
                    if tx.send(ws_msg.clone()).await.is_err() {
                        tracing::warn!(
                            "DND: Failed to broadcast generic message to admin client {}",
                            id
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    "DND: Failed to serialize generic message for broadcast: {}",
                    e
                );
            }
        }
    }

    // --- Game Logic Methods (largely the same, but use new types and broadcast methods) ---

    fn construct_full_game_state_payload(&self) -> DNDFullGameState {
        let (tag, desc, admin_instr) = self.get_internal_state_tag_and_desc();
        let client_briefcases = self
            .briefcases
            .iter()
            .map(|b| DNDBriefcaseClientInfo {
                id: b.id,
                is_opened: b.is_opened,
                value: if b.is_opened { Some(b.value) } else { None },
                is_player_case: Some(b.id) == self.player_chosen_case_id,
            })
            .collect();

        let mut vote_tally = None;
        if self.is_voting_active {
            if let Some(context) = &self.current_vote_context {
                vote_tally = Some(self.tally_current_votes(context));
            }
        }

        let (current_round, cases_to_open_this_round, cases_opened_in_current_round) =
            match &self.internal_state {
                DNDInternalState::PlayerCaseSelectedMovingToRound { round_num } => {
                    // This state is transitional. The *next* round's 'to_open' count might be relevant
                    // or 0 if it's not yet determined/applicable for client display.
                    // Let's assume 0 for now, or derive from ROUND_SCHEDULE[0] if round_num is 1.
                    let to_open_next = if *round_num == 1 && !ROUND_SCHEDULE.is_empty() {
                        ROUND_SCHEDULE[0]
                    } else {
                        0
                    };
                    (*round_num, to_open_next, 0)
                }
                DNDInternalState::ReadyForRoundStart {
                    round_num,
                    num_to_open_in_round,
                } => (*round_num, *num_to_open_in_round, 0),
                DNDInternalState::RoundCaseOpeningVoting {
                    round_num,
                    num_to_open_in_round,
                    cases_chosen_for_opening_count,
                } => (
                    *round_num,
                    *num_to_open_in_round,
                    *cases_chosen_for_opening_count,
                ),
                DNDInternalState::CasesOpenedMovingToOffer { round_num }
                | DNDInternalState::BankerOfferPresented { round_num, .. }
                | DNDInternalState::DealNoDealVoting { round_num, .. } => (*round_num, 0, 0),
                _ => (0, 0, 0),
            };

        DNDFullGameState {
            internal_state_tag: tag,
            current_phase_description: desc,
            admin_instructions: admin_instr,
            briefcases: client_briefcases,
            player_chosen_case_id: self.player_chosen_case_id,
            remaining_money_values: self.remaining_money_values_in_play.clone(),
            current_round,
            cases_to_open_this_round,
            cases_opened_in_current_round,
            banker_offer: self.last_banker_offer,
            current_vote_tally: vote_tally,
        }
    }

    fn get_internal_state_tag_and_desc(&self) -> (String, String, String) {
        // (This method's logic remains the same as your previous version, just ensure
        // the admin instructions refer to the DNDAdminCommand variants, e.g., "StartGame")
        match &self.internal_state {
            DNDInternalState::NotStarted => (
                "NotStarted".to_string(),
                "Game has not started.".to_string(),
                "Admin: Send DNDAdminCommand::StartGame to begin.".to_string(),
            ),
            DNDInternalState::GameInitializing => (
                "GameInitializing".to_string(),
                "Game is setting up briefcases...".to_string(),
                "Please wait.".to_string(),
            ),
            DNDInternalState::ReadyForPlayerCaseSelection => (
                "ReadyForPlayerCaseSelection".to_string(),
                "Game ready. Twitch needs to select their briefcase.".to_string(),
                "Admin: Send DNDAdminCommand::StartPlayerCaseSelectionVote.".to_string(),
            ),
            DNDInternalState::PlayerCaseSelectionVoting => (
                "PlayerCaseSelectionVoting".to_string(),
                format!(
                    "Twitch is voting for their initial briefcase (1-{}).",
                    ACTUAL_TOTAL_CASES
                ),
                "Admin: Send DNDAdminCommand::ConcludeVotingAndProcess.".to_string(),
            ),
            DNDInternalState::PlayerCaseSelectedMovingToRound { round_num } => (
                "PlayerCaseSelectedMovingToRound".to_string(),
                format!("Player case selected. Preparing for Round {}.", round_num),
                "Transitioning...".to_string(),
            ),
            DNDInternalState::ReadyForRoundStart {
                round_num,
                num_to_open_in_round,
            } => (
                "ReadyForRoundStart".to_string(),
                format!(
                    "Round {}: Ready to select {} case(s) to open.",
                    round_num, num_to_open_in_round
                ),
                "Admin: Send DNDAdminCommand::StartRoundCaseOpeningVote.".to_string(),
            ),
            DNDInternalState::RoundCaseOpeningVoting {
                round_num,
                num_to_open_in_round,
                cases_chosen_for_opening_count,
            } => (
                "RoundCaseOpeningVoting".to_string(),
                format!(
                    "Round {}: Twitch voting. Need to select {} more case(s) out of {} to open.",
                    round_num,
                    num_to_open_in_round - cases_chosen_for_opening_count,
                    num_to_open_in_round
                ),
                "Admin: Send DNDAdminCommand::ConcludeVotingAndProcess.".to_string(),
            ),
            DNDInternalState::CasesOpenedMovingToOffer { round_num } => (
                "CasesOpenedMovingToOffer".to_string(),
                format!(
                    "Round {} cases opened. Calculating Banker's offer...",
                    round_num
                ),
                "Please wait.".to_string(),
            ),
            DNDInternalState::BankerOfferPresented { round_num, offer } => (
                "BankerOfferPresented".to_string(),
                format!("Round {}: Banker offers ${:?}.", round_num, offer),
                "Admin: Send DNDAdminCommand::StartDealNoDealVote.".to_string(),
            ),
            DNDInternalState::DealNoDealVoting { round_num, offer } => (
                "DealNoDealVoting".to_string(),
                format!(
                    "Round {}: Banker's offer is ${:?}. Twitch voting 'Deal' or 'No Deal'.",
                    round_num, offer
                ),
                "Admin: Send DNDAdminCommand::ConcludeVotingAndProcess.".to_string(),
            ),
            DNDInternalState::GameEndedDeal {
                winnings,
                player_case_value,
            } => (
                "GameEndedDeal".to_string(),
                format!(
                    "DEAL! Twitch won ${:?}. Their case had ${:?}.",
                    winnings, player_case_value
                ),
                "Game over. Admin: Send DNDAdminCommand::StartGame to play again.".to_string(),
            ),
            DNDInternalState::GameEndedNoDeal { winnings } => (
                "GameEndedNoDeal".to_string(),
                format!("NO DEAL! Twitch won ${:?} from their case.", winnings),
                "Game over. Admin: Send DNDAdminCommand::StartGame to play again.".to_string(),
            ),
        }
    }

    fn initialize_game_board(&mut self) {
        let mut money = ACTUAL_MONEY_VALUES.to_vec();
        money.shuffle(&mut thread_rng());

        self.briefcases = (1..=ACTUAL_TOTAL_CASES)
            .zip(money.into_iter())
            .map(|(id, value)| Briefcase {
                id,
                value,
                is_opened: false,
                is_player_case: false,
            })
            .collect();
        self.remaining_money_values_in_play = ACTUAL_MONEY_VALUES.to_vec();
        self.remaining_money_values_in_play.sort_unstable(); // Keep sorted for display
        self.player_chosen_case_id = None;
        self.is_voting_active = false;
        self.current_vote_context = None;
        self.current_votes_by_user.clear();
        self.current_round_index = 0;
        self.last_banker_offer = None;
        self.internal_state = DNDInternalState::ReadyForPlayerCaseSelection;
    }

    fn start_voting_period(&mut self, context: DNDVoteTypeContext) {
        self.is_voting_active = true;
        self.current_vote_context = Some(context);
        self.current_votes_by_user.clear();
    }

    fn tally_current_votes(&self, context: &DNDVoteTypeContext) -> HashMap<String, u32> {
        let mut tally: HashMap<String, u32> = HashMap::new();
        for vote_str in self.current_votes_by_user.values() {
            match context {
                DNDVoteTypeContext::SelectPlayerCase
                | DNDVoteTypeContext::OpenRoundCases { .. } => {
                    // Assuming vote_str is already validated to be a number string
                    *tally.entry(vote_str.clone()).or_insert(0) += 1;
                }
                DNDVoteTypeContext::DealOrNoDeal => {
                    // Assuming vote_str is already "DEAL" or "NO DEAL"
                    *tally.entry(vote_str.clone()).or_insert(0) += 1;
                }
            }
        }
        tally
    }

    fn process_player_case_selection_votes(&mut self) -> Option<u8> {
        if let Some(DNDVoteTypeContext::SelectPlayerCase) = self.current_vote_context {
            let tally = self.tally_current_votes(&DNDVoteTypeContext::SelectPlayerCase);
            tally
                .into_iter()
                // Find max count, then smallest case ID on tie
                .max_by(|a, b| {
                    a.1.cmp(&b.1).then_with(|| {
                        b.0.parse::<u8>()
                            .unwrap_or(u8::MAX)
                            .cmp(&a.0.parse::<u8>().unwrap_or(u8::MAX))
                    })
                })
                .and_then(|(case_id_str, _count)| case_id_str.parse::<u8>().ok())
        } else {
            None
        }
    }

    fn process_round_case_opening_votes(&mut self, num_to_select: u8) -> Vec<u8> {
        if let Some(DNDVoteTypeContext::OpenRoundCases { .. }) = self.current_vote_context {
            let tally = self.tally_current_votes(&self.current_vote_context.unwrap());
            let mut sorted_votes: Vec<(u8, u32)> = tally
                .into_iter()
                .filter_map(|(id_str, count)| id_str.parse::<u8>().ok().map(|id| (id, count)))
                // Ensure case is available (not opened, not player's case)
                .filter(|(id, _)| {
                    self.briefcases.iter().any(|b| {
                        b.id == *id && !b.is_opened && Some(*id) != self.player_chosen_case_id
                    })
                })
                .collect();

            // Sort by highest vote count, then by smallest case ID for ties
            sorted_votes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            sorted_votes
                .into_iter()
                .map(|(id, _)| id)
                .take(num_to_select as usize)
                .collect()
        } else {
            Vec::new()
        }
    }

    fn process_deal_no_deal_votes(&mut self) -> Option<bool> {
        // true for Deal, false for No Deal
        if let Some(DNDVoteTypeContext::DealOrNoDeal) = self.current_vote_context {
            let tally = self.tally_current_votes(&DNDVoteTypeContext::DealOrNoDeal);
            let deal_votes = tally.get("DEAL").cloned().unwrap_or(0);
            let no_deal_votes = tally.get("NO DEAL").cloned().unwrap_or(0);

            if deal_votes == 0 && no_deal_votes == 0 {
                return Some(false);
            } // Default to No Deal if no valid votes
            Some(deal_votes >= no_deal_votes) // Tie goes to Deal (or adjust as preferred)
        } else {
            None
        }
    }

    fn open_briefcase(&mut self, case_id_to_open: u8) -> Option<u64> {
        if let Some(case) = self.briefcases.iter_mut().find(|b| b.id == case_id_to_open) {
            if !case.is_opened {
                case.is_opened = true;
                // Remove value from remaining_money_values_in_play
                if let Some(pos) = self
                    .remaining_money_values_in_play
                    .iter()
                    .position(|&v| v == case.value)
                {
                    self.remaining_money_values_in_play.remove(pos);
                }
                // self.remaining_money_values_in_play.sort_unstable(); // Keep sorted for display
                return Some(case.value);
            }
        }
        None
    }

    fn calculate_banker_offer(&mut self) -> u64 {
        if self.remaining_money_values_in_play.is_empty() {
            self.last_banker_offer = Some(0);
            return 0;
        }
        let sum: u64 = self.remaining_money_values_in_play.iter().sum();
        let avg = sum as f64 / self.remaining_money_values_in_play.len() as f64;
        // Banker's offer logic: e.g., average * round factor
        let round_factor = 0.65 + (self.current_round_index as f64 * 0.05); // Increases from 0.65 to ~0.9 for 6 rounds
        let offer = (avg * round_factor.min(0.92)).round() as u64; // Cap factor
        self.last_banker_offer = Some(offer);
        offer
    }
    // ... (rest of the game logic methods: initialize_game_board, start_voting_period, etc.)
    // Ensure they use the new types and call `broadcast_dnd_event_to_all_admins` with `DNDGameEvent` variants.
}

impl GameLogic for DealNoDealGame {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        tracing::info!("DNDGame: Admin client {} connected.", client_id);
        self.clients.insert(client_id.clone(), client_tx);
        // Send current game state to the newly connected admin
        let initial_state_payload = self.construct_full_game_state_payload();
        self.send_dnd_event_to_client(
            &client_id,
            DNDGameEvent::GameStateUpdate(initial_state_payload),
        )
        .await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        tracing::info!("DNDGame: Admin client {} disconnected.", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(
        &mut self,
        client_id: Uuid, // ID of the admin client sending the command
        message: GenericClientToServerMessage,
    ) {
        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
                // game_instance_id, // Use if you have this
            } => {
                if game_type_id != self.game_type_id() {
                    tracing::warn!(
                        "DNDGame: Received command for wrong game type: {}. Expected: {}",
                        game_type_id,
                        self.game_type_id()
                    );
                    // Optionally send a system error back to the client
                    let err_msg = GenericServerToClientMessage::SystemError {
                        message: format!(
                            "Command intended for game type '{}', but this is '{}'.",
                            game_type_id,
                            self.game_type_id()
                        ),
                    };
                    self.send_generic_message_to_client(&client_id, err_msg)
                        .await;
                    return;
                }

                // Attempt to deserialize command_data into DNDAdminCommand
                match serde_json::from_value::<DNDAdminCommand>(command_data.clone()) {
                    Ok(dnd_command) => {
                        // --- Begin DNDAdminCommand specific logic ---
                        match dnd_command {
                            DNDAdminCommand::StartGame => {
                                if matches!(
                                    self.internal_state,
                                    DNDInternalState::NotStarted
                                        | DNDInternalState::GameEndedDeal { .. }
                                        | DNDInternalState::GameEndedNoDeal { .. }
                                ) {
                                    self.internal_state = DNDInternalState::GameInitializing;
                                    self.initialize_game_board();
                                    tracing::info!(
                                        "DNDGame: Game started and initialized by admin {}.",
                                        client_id
                                    );
                                } else {
                                    tracing::warn!(
                                        "DNDGame: Admin {} tried StartGame in invalid state: {:?}",
                                        client_id,
                                        self.internal_state
                                    );
                                }
                            }
                            DNDAdminCommand::StartPlayerCaseSelectionVote => {
                                if self.internal_state
                                    == DNDInternalState::ReadyForPlayerCaseSelection
                                {
                                    self.internal_state =
                                        DNDInternalState::PlayerCaseSelectionVoting;
                                    let context = DNDVoteTypeContext::SelectPlayerCase;
                                    self.start_voting_period(context);
                                    self.broadcast_dnd_event_to_all_admins(DNDGameEvent::VotingPeriodChange {
                                        is_active: true, vote_context: context,
                                        instruction_or_outcome: format!("Twitch: Vote for your starting briefcase (1-{}). Type the number in chat!", ACTUAL_TOTAL_CASES),
                                        final_tally: None,
                                    }).await;
                                    tracing::info!("DNDGame: Player case selection voting started by admin {}.", client_id);
                                } else {
                                    tracing::warn!("DNDGame: Admin {} tried StartPlayerCaseSelectionVote in invalid state: {:?}", client_id, self.internal_state);
                                }
                            }
                            DNDAdminCommand::StartRoundCaseOpeningVote => {
                                if let DNDInternalState::ReadyForRoundStart {
                                    round_num,
                                    num_to_open_in_round,
                                } = self.internal_state
                                {
                                    self.internal_state =
                                        DNDInternalState::RoundCaseOpeningVoting {
                                            round_num,
                                            num_to_open_in_round,
                                            cases_chosen_for_opening_count: 0,
                                        };
                                    let context = DNDVoteTypeContext::OpenRoundCases {
                                        num_expected: num_to_open_in_round,
                                    };
                                    self.start_voting_period(context);
                                    self.broadcast_dnd_event_to_all_admins(DNDGameEvent::VotingPeriodChange {
                                        is_active: true, vote_context: context,
                                        instruction_or_outcome: format!("Twitch: Vote for {} case(s) to open this round. Type numbers in chat!", num_to_open_in_round),
                                        final_tally: None,
                                    }).await;
                                    tracing::info!("DNDGame: Round {} case opening voting started by admin {}.", round_num, client_id);
                                } else {
                                    tracing::warn!("DNDGame: Admin {} tried StartRoundCaseOpeningVote in invalid state: {:?}", client_id, self.internal_state);
                                }
                            }
                            DNDAdminCommand::StartDealNoDealVote => {
                                if let DNDInternalState::BankerOfferPresented { round_num, offer } =
                                    self.internal_state
                                {
                                    self.internal_state =
                                        DNDInternalState::DealNoDealVoting { round_num, offer };
                                    let context = DNDVoteTypeContext::DealOrNoDeal;
                                    self.start_voting_period(context);
                                    self.broadcast_dnd_event_to_all_admins(DNDGameEvent::VotingPeriodChange {
                                        is_active: true, vote_context: context,
                                        instruction_or_outcome: format!("Banker offers ${}. Twitch: Vote 'Deal' or 'No Deal'!", offer),
                                        final_tally: None,
                                    }).await;
                                    tracing::info!("DNDGame: Deal/No Deal voting started by admin {} for offer ${}.", client_id, offer);
                                } else {
                                    tracing::warn!("DNDGame: Admin {} tried StartDealNoDealVote in invalid state: {:?}", client_id, self.internal_state);
                                }
                            }
                            DNDAdminCommand::ConcludeVotingAndProcess => {
                                if !self.is_voting_active {
                                    tracing::warn!("DNDGame: Admin {} tried ConcludeVotingAndProcess but no voting is active.", client_id);
                                    self.broadcast_current_full_game_state().await; // Send current state again
                                    return;
                                }
                                tracing::info!("DNDGame: Voting concluded by admin {}.", client_id);
                                self.is_voting_active = false; // Stop accepting new Twitch votes

                                let context = self
                                    .current_vote_context
                                    .take()
                                    .expect("Voting context missing during conclusion");
                                let final_tally = self.tally_current_votes(&context);

                                match context {
                                    DNDVoteTypeContext::SelectPlayerCase => {
                                        if let Some(selected_case_id) =
                                            self.process_player_case_selection_votes()
                                        {
                                            self.player_chosen_case_id = Some(selected_case_id);
                                            if let Some(case) = self
                                                .briefcases
                                                .iter_mut()
                                                .find(|b| b.id == selected_case_id)
                                            {
                                                case.is_player_case = true;
                                            }
                                            self.internal_state =
                                                DNDInternalState::PlayerCaseSelectedMovingToRound {
                                                    round_num: 1,
                                                };
                                            self.current_round_index = 0; // Start with the first round schedule
                                            self.broadcast_dnd_event_to_all_admins(DNDGameEvent::VotingPeriodChange {
                                                is_active: false, vote_context: context,
                                                instruction_or_outcome: format!("Voting ended. Twitch selected briefcase #{} as their case!", selected_case_id),
                                                final_tally: Some(final_tally),
                                            }).await;
                                            // Auto-transition to ReadyForRoundStart
                                            let num_to_open =
                                                ROUND_SCHEDULE[self.current_round_index];
                                            self.internal_state =
                                                DNDInternalState::ReadyForRoundStart {
                                                    round_num: 1,
                                                    num_to_open_in_round: num_to_open,
                                                };
                                        } else {
                                            // No valid votes, or error. Re-enable voting or ask admin.
                                            self.start_voting_period(context); // Re-open voting
                                            self.broadcast_dnd_event_to_all_admins(DNDGameEvent::VotingPeriodChange {
                                                is_active: true, vote_context: context, // Mark as active again
                                                instruction_or_outcome: "Voting ended but no valid case selected. Voting re-opened. Please vote again!".to_string(),
                                                final_tally: Some(final_tally), // Show previous tally
                                            }).await;
                                        }
                                    }
                                    DNDVoteTypeContext::OpenRoundCases { num_expected } => {
                                        // Ensure we are in the correct internal state before proceeding
                                        let round_num = match self.internal_state {
                                            DNDInternalState::RoundCaseOpeningVoting {
                                                round_num,
                                                ..
                                            } => round_num,
                                            _ => {
                                                tracing::error!("DNDGame: ConcludeVoting for OpenRoundCases called in unexpected state: {:?}", self.internal_state);
                                                self.broadcast_current_full_game_state().await;
                                                return;
                                            }
                                        };

                                        let cases_to_open_ids =
                                            self.process_round_case_opening_votes(num_expected);
                                        let mut outcome_desc = format!(
                                            "Voting ended for Round {}. Cases opened: ",
                                            round_num
                                        );

                                        if cases_to_open_ids.is_empty() && num_expected > 0 {
                                            outcome_desc = format!("Voting ended for Round {}. No valid cases selected to open. Voting re-opened.", round_num);
                                            self.start_voting_period(context); // Re-open voting
                                            self.broadcast_dnd_event_to_all_admins(
                                                DNDGameEvent::VotingPeriodChange {
                                                    is_active: true,
                                                    vote_context: context,
                                                    instruction_or_outcome: outcome_desc.clone(),
                                                    final_tally: Some(final_tally),
                                                },
                                            )
                                            .await;
                                        } else {
                                            for (i, case_id) in cases_to_open_ids.iter().enumerate()
                                            {
                                                if let Some(value_opened) =
                                                    self.open_briefcase(*case_id)
                                                {
                                                    tracing::info!(
                                                        "DNDGame: Case {} opened with value ${}",
                                                        case_id,
                                                        value_opened
                                                    );
                                                    self.broadcast_dnd_event_to_all_admins(
                                                        DNDGameEvent::CaseOpened {
                                                            case_id: *case_id,
                                                            value: value_opened,
                                                            is_player_case_reveal_at_end: false,
                                                        },
                                                    )
                                                    .await;
                                                    if i > 0 {
                                                        outcome_desc.push_str(", ");
                                                    }
                                                    outcome_desc.push_str(&format!(
                                                        "#{} (${})",
                                                        case_id, value_opened
                                                    ));
                                                }
                                            }
                                            if cases_to_open_ids.is_empty() && num_expected > 0 {
                                                // only if we expected to open some
                                                outcome_desc.push_str("None (no valid votes or all available were opened).");
                                            }

                                            self.broadcast_dnd_event_to_all_admins(
                                                DNDGameEvent::VotingPeriodChange {
                                                    is_active: false,
                                                    vote_context: context,
                                                    instruction_or_outcome: outcome_desc,
                                                    final_tally: Some(final_tally),
                                                },
                                            )
                                            .await;

                                            // Check if game ends (only player case and one other left, or just player case)
                                            let unopened_not_player_case_count = self
                                                .briefcases
                                                .iter()
                                                .filter(|b| {
                                                    !b.is_opened
                                                        && Some(b.id) != self.player_chosen_case_id
                                                })
                                                .count();

                                            if unopened_not_player_case_count == 0
                                                && self.player_chosen_case_id.is_some()
                                            {
                                                let player_case = self
                                                    .briefcases
                                                    .iter()
                                                    .find(|b| {
                                                        Some(b.id) == self.player_chosen_case_id
                                                    })
                                                    .expect(
                                                        "Player case ID set but case not found",
                                                    );
                                                self.internal_state =
                                                    DNDInternalState::GameEndedNoDeal {
                                                        winnings: player_case.value,
                                                    };
                                                self.broadcast_dnd_event_to_all_admins(
                                                    DNDGameEvent::CaseOpened {
                                                        case_id: player_case.id,
                                                        value: player_case.value,
                                                        is_player_case_reveal_at_end: true,
                                                    },
                                                )
                                                .await;
                                                self.broadcast_dnd_event_to_all_admins(DNDGameEvent::GameEnded {
                                                    summary: format!("No Deal! Game ended. Player case #{} had ${}", player_case.id, player_case.value),
                                                    winnings: player_case.value, player_case_original_value: player_case.value,
                                                }).await;
                                            } else {
                                                // Proceed to banker offer
                                                self.internal_state =
                                                    DNDInternalState::CasesOpenedMovingToOffer {
                                                        round_num,
                                                    };
                                                let offer = self.calculate_banker_offer();
                                                self.broadcast_dnd_event_to_all_admins(
                                                    DNDGameEvent::BankerOffer {
                                                        offer_amount: offer,
                                                    },
                                                )
                                                .await;
                                                self.internal_state =
                                                    DNDInternalState::BankerOfferPresented {
                                                        round_num,
                                                        offer,
                                                    };
                                            }
                                        }
                                    }
                                    DNDVoteTypeContext::DealOrNoDeal => {
                                        let (round_num, offer) = match self.internal_state {
                                            DNDInternalState::DealNoDealVoting {
                                                round_num,
                                                offer,
                                            } => (round_num, offer),
                                            _ => {
                                                tracing::error!("DNDGame: ConcludeVoting for DealNoDeal called in unexpected state: {:?}", self.internal_state);
                                                self.broadcast_current_full_game_state().await;
                                                return;
                                            }
                                        };
                                        let player_took_deal =
                                            self.process_deal_no_deal_votes().unwrap_or(false); // Default No Deal on error/tie

                                        let player_case_id_unwrapped = self
                                            .player_chosen_case_id
                                            .expect("Player case ID must be set by now");
                                        let player_case_value = self
                                            .briefcases
                                            .iter()
                                            .find(|b| b.id == player_case_id_unwrapped)
                                            .map_or(0, |c| c.value);

                                        self.broadcast_dnd_event_to_all_admins(
                                            DNDGameEvent::VotingPeriodChange {
                                                is_active: false,
                                                vote_context: context,
                                                instruction_or_outcome: format!(
                                                    "Voting ended. Twitch chose: {}!",
                                                    if player_took_deal {
                                                        "DEAL"
                                                    } else {
                                                        "NO DEAL"
                                                    }
                                                ),
                                                final_tally: Some(final_tally),
                                            },
                                        )
                                        .await;

                                        if player_took_deal {
                                            self.internal_state = DNDInternalState::GameEndedDeal {
                                                winnings: offer,
                                                player_case_value,
                                            };
                                            self.broadcast_dnd_event_to_all_admins(DNDGameEvent::GameEnded {
                                                summary: format!("DEAL! Twitch won ${}. Their case #{} had ${}", offer, player_case_id_unwrapped, player_case_value),
                                                winnings: offer, player_case_original_value: player_case_value,
                                            }).await;
                                        } else {
                                            // No Deal
                                            self.current_round_index += 1;
                                            // Check if it's the end of all scheduled rounds or no more cases to open other than player's
                                            let unopened_not_player_case_count = self
                                                .briefcases
                                                .iter()
                                                .filter(|b| {
                                                    !b.is_opened
                                                        && Some(b.id) != self.player_chosen_case_id
                                                })
                                                .count();

                                            if self.current_round_index >= ROUND_SCHEDULE.len()
                                                || unopened_not_player_case_count == 0
                                            {
                                                self.internal_state =
                                                    DNDInternalState::GameEndedNoDeal {
                                                        winnings: player_case_value,
                                                    };
                                                self.broadcast_dnd_event_to_all_admins(
                                                    DNDGameEvent::CaseOpened {
                                                        case_id: player_case_id_unwrapped,
                                                        value: player_case_value,
                                                        is_player_case_reveal_at_end: true,
                                                    },
                                                )
                                                .await;
                                                self.broadcast_dnd_event_to_all_admins(DNDGameEvent::GameEnded {
                                                    summary: format!("NO DEAL! Game ended. Player case #{} had ${}", player_case_id_unwrapped, player_case_value),
                                                    winnings: player_case_value, player_case_original_value: player_case_value,
                                                }).await;
                                            } else {
                                                // Proceed to next round
                                                let next_round_num = round_num + 1;
                                                let num_to_open_scheduled =
                                                    ROUND_SCHEDULE[self.current_round_index];
                                                // Ensure we don't try to open more than available
                                                let actual_num_to_open = std::cmp::min(
                                                    num_to_open_scheduled,
                                                    unopened_not_player_case_count as u8,
                                                );

                                                if actual_num_to_open > 0 {
                                                    self.internal_state =
                                                        DNDInternalState::ReadyForRoundStart {
                                                            round_num: next_round_num,
                                                            num_to_open_in_round:
                                                                actual_num_to_open,
                                                        };
                                                } else {
                                                    // Should only happen if unopened_not_player_case_count was 0, covered above. Safety.
                                                    self.internal_state =
                                                        DNDInternalState::GameEndedNoDeal {
                                                            winnings: player_case_value,
                                                        };
                                                    self.broadcast_dnd_event_to_all_admins(
                                                        DNDGameEvent::CaseOpened {
                                                            case_id: player_case_id_unwrapped,
                                                            value: player_case_value,
                                                            is_player_case_reveal_at_end: true,
                                                        },
                                                    )
                                                    .await;
                                                    self.broadcast_dnd_event_to_all_admins(DNDGameEvent::GameEnded {
                                                       summary: format!("NO DEAL! (Error state) Player case #{} had ${}", player_case_id_unwrapped, player_case_value),
                                                       winnings: player_case_value, player_case_original_value: player_case_value,
                                                   }).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // --- End DNDAdminCommand specific logic ---
                        // After any successful command processing, broadcast the new full state.
                        self.broadcast_current_full_game_state().await;
                    }
                    Err(e) => {
                        tracing::error!(
                            "DNDGame: Failed to deserialize DNDAdminCommand from client {}: {}. Payload: {:?}",
                            client_id, e, command_data
                        );
                        // Send a system error back to the specific admin client
                        let err_msg = GenericServerToClientMessage::SystemError {
                            message: format!("Invalid DealNoDeal command format: {}. Ensure your command matches DNDAdminCommand structure.", e)
                        };
                        self.send_generic_message_to_client(&client_id, err_msg)
                            .await;
                    }
                }
            }
            GenericClientToServerMessage::GlobalCommand { command_name, data } => {
                tracing::debug!(
                    "DNDGame: Received GlobalCommand (unhandled by DND): name {}, data {:?}, from client {}",
                    command_name, data, client_id
                );
                // This game type might not handle global commands, or only specific ones.
                // The Lobby/GameManager should ideally handle these before routing to a game.
            }
        }
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        if !self.is_voting_active || self.current_vote_context.is_none() {
            return; // Not in a voting phase, or context not set
        }

        let vote_text = message.text.trim();
        let voter_id = message.sender_username; // Twitch username
        let context = self.current_vote_context.unwrap(); // Known to be Some due to is_voting_active

        let mut parsed_vote_value: Option<String> = None; // Stores the canonical form, e.g. "15", "DEAL"
        let mut is_valid_vote_for_context = false;

        match context {
            DNDVoteTypeContext::SelectPlayerCase | DNDVoteTypeContext::OpenRoundCases { .. } => {
                if let Ok(case_id_vote) = vote_text.parse::<u8>() {
                    if case_id_vote >= 1 && case_id_vote <= ACTUAL_TOTAL_CASES {
                        // Additional check: case must be available (not opened, not player's case if opening round)
                        let case_is_choosable = self.briefcases.iter().any(|b| {
                            b.id == case_id_vote && !b.is_opened &&
                            // For OpenRoundCases, can't pick the player's chosen case
                            (context == DNDVoteTypeContext::SelectPlayerCase || Some(b.id) != self.player_chosen_case_id)
                        });

                        if case_is_choosable {
                            is_valid_vote_for_context = true;
                            parsed_vote_value = Some(case_id_vote.to_string());
                        }
                    }
                }
            }
            DNDVoteTypeContext::DealOrNoDeal => {
                let lower_vote = vote_text.to_lowercase();
                if lower_vote == "deal" || lower_vote == "yes" {
                    is_valid_vote_for_context = true;
                    parsed_vote_value = Some("DEAL".to_string());
                } else if lower_vote == "no" || lower_vote == "nodeal" {
                    is_valid_vote_for_context = true;
                    parsed_vote_value = Some("NO DEAL".to_string());
                }
            }
        }

        if is_valid_vote_for_context {
            // Store the canonical parsed vote value
            self.current_votes_by_user
                .insert(voter_id.clone(), parsed_vote_value.clone().unwrap());
        }

        // Notify admins about the received Twitch vote attempt
        let dnd_event = DNDGameEvent::TwitchVoteReceived {
            voter_twitch_username: voter_id,
            raw_vote_text: vote_text.to_string(),
            is_valid_vote: is_valid_vote_for_context, // Reflects if it was valid *for the current context*
            parsed_vote_value,                        // Send the parsed value if valid, or None
            vote_context: context,
        };
        self.broadcast_dnd_event_to_all_admins(dnd_event).await;

        // If a valid vote was registered, update the game state to reflect new tally for admins
        if is_valid_vote_for_context {
            self.broadcast_current_full_game_state().await;
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
