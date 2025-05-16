// src/game_logic/deal_no_deal_game/types.rs
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
