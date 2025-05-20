// --- Mirroring Server Enums and Structs ---

export interface AdminCommand {
	command: 'StartGame' | 'ConcludeVotingAndProcess';
	// No content needed for these commands as per server
}

// GamePhase enum from server
// The string values in the union should exactly match the Rust enum variant names.
export type GamePhaseType =
	| { type: 'Setup' }
	| { type: 'PlayerCaseSelection_Voting' }
	| {
			type: 'RoundCaseOpening_Voting';
			round_number: number; // u8
			total_to_open_for_round: number; // u8
			opened_so_far_for_round: number; // u8
	  }
	| {
			type: 'BankerOfferCalculation';
			round_number: number; // u8
	  }
	| {
			type: 'DealOrNoDeal_Voting';
			round_number: number; // u8
			offer: number; // u64
	  }
	| {
			type: 'GameOver';
			summary: string;
			winnings: number; // u64
			player_case_original_value: number; //u64
	  };

// DealNoDealGame struct from server (client-side representation)
// This is what we receive in FullStateUpdate
export interface DealNoDealGameState {
	phase: GamePhaseType;
	briefcase_values: number[]; // Vec<u64> -> number[]
	briefcase_is_opened: boolean[]; // Vec<bool>
	player_chosen_case_index: number | null; // Option<usize>
	remaining_money_values_in_play: number[]; // Vec<u64>
	current_round_schedule_index: number; // usize

	// Fields populated by prepare_for_client_view on server
	current_round_display_number: number; // u8
	cases_to_open_this_round_target: number; // u8
	cases_opened_in_current_round_segment: number; // u8
	banker_offer: number | null; // Option<u64>
	current_vote_tally: Record<string, number> | null; // Option<HashMap<String, u32>> -> Record<string, number> | null
}

// GameEvent enum from server (client-side representation of event_data)
export type GameEventData =
	| { event_type: 'FullStateUpdate'; data: DealNoDealGameState }
	| {
			event_type: 'PlayerVoteRegistered';
			data: {
				voter_username: string;
				vote_value: string; // This is the raw vote (e.g., "15", "DEAL")
			};
	  }
	| {
			event_type: 'CaseOpened';
			data: {
				case_index: number; // usize
				value: number; // u64
				is_player_case_reveal_at_end: boolean;
			};
	  }
	| {
			event_type: 'BankerOfferPresented';
			data: {
				offer_amount: number; // u64
			};
	  };

// Type for the command_data part of GameSpecificCommandPayload when game_type_id is 'DealNoDeal'
// This directly matches the AdminCommand enum on the server (just the tag).
export type DealNoDealCommandData = AdminCommand;
