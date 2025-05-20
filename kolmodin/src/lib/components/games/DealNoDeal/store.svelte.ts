import { registerGameStore } from '$lib/services/game.event.router';
import { websocketStore } from '$lib/stores/websocket.store.svelte';
import type { GameSpecificCommandPayload } from '$lib/types/websocket.types';
import type {
	DealNoDealGameState,
	GameEventData,
	DealNoDealCommandData,
	GamePhaseType
} from './types';
import { debug, warn, info } from '$lib/utils/logger';

const GAME_TYPE_ID = 'DealNoDeal';

interface DealNoDealStoreActions {
	startGame: () => void;
	concludeVotingAndProcess: () => void;
}

// Helper to create a default/initial state
function createInitialDndState(): DealNoDealGameState {
	return {
		phase: { type: 'Setup' },
		briefcase_values: [],
		briefcase_is_opened: [],
		player_chosen_case_index: null,
		remaining_money_values_in_play: [],
		current_round_schedule_index: 0,
		current_round_display_number: 0,
		cases_to_open_this_round_target: 0,
		cases_opened_in_current_round_segment: 0,
		banker_offer: null,
		current_vote_tally: null
	};
}

function createDealNoDealStore() {
	const gameState = $state<DealNoDealGameState>(createInitialDndState());

	// We can also have a piece of state for live vote feed that is not part of FullStateUpdate
	const liveVoteFeed = $state<{ voter_username: string; vote_value: string }[]>([]);
	const MAX_VOTE_FEED_ITEMS = 20;

	function processEvent(eventPayload: GameEventData): void {
		debug(`DealNoDealStore: Processing event type "${eventPayload.event_type}"`, eventPayload.data);
		switch (eventPayload.event_type) {
			case 'FullStateUpdate':
				// The server sends the complete, prepared state.
				// Replace the client's state entirely with this.
				Object.assign(gameState, eventPayload.data);
				info('DealNoDealStore: Full state updated.');
				// Clear live vote feed on full state update as tally is now included
				liveVoteFeed.length = 0;
				break;
			case 'PlayerVoteRegistered':
				// Add to the live feed for immediate UI feedback
				liveVoteFeed.unshift(eventPayload.data); // Add to the beginning
				if (liveVoteFeed.length > MAX_VOTE_FEED_ITEMS) {
					liveVoteFeed.pop(); // Keep the list trimmed
				}
				// The server's FullStateUpdate will eventually include the official tally.
				// No need to update gameState.current_vote_tally here.
				break;
			case 'CaseOpened':
				// The server will send a FullStateUpdate after this if needed for board changes.
				// For immediate feedback, we could update the specific case, but it risks divergence
				// if the FullStateUpdate logic is complex.
				// For now, rely on FullStateUpdate, but log this event.
				info(
					`DealNoDealStore: CaseOpened event received for case ${eventPayload.data.case_index + 1} with value ${eventPayload.data.value}`
				);
				// Optionally, show a temporary notification or animation based on this.
				break;
			case 'BankerOfferPresented':
				// Similar to CaseOpened, the server might send FullStateUpdate.
				// If not, and only `banker_offer` needs updating, we can do it here.
				// The server's GamePhase::DealOrNoDeal_Voting includes the offer, so FullStateUpdate handles it.
				info(
					`DealNoDealStore: BankerOfferPresented event received: ${eventPayload.data.offer_amount}`
				);
				// gameState.banker_offer = eventPayload.data.offer_amount; // Only if FullStateUpdate doesn't cover this.
				break;
			default:
				warn(`DealNoDealStore: Unhandled event type: ${(eventPayload as any).event_type}`);
		}
	}

	function sendCommand(command: DealNoDealCommandData['command']): void {
		// Command data for DND is just the command name (tag)
		const commandData: DealNoDealCommandData = { command };
		const payload: GameSpecificCommandPayload = {
			game_type_id: GAME_TYPE_ID,
			command_data: commandData
		};
		websocketStore.send({ message_type: 'GameSpecificCommand', payload });
	}

	const actions: DealNoDealStoreActions = {
		startGame: () => sendCommand('StartGame'),
		concludeVotingAndProcess: () => sendCommand('ConcludeVotingAndProcess')
	};

	registerGameStore(GAME_TYPE_ID, { processEvent });

	return {
		get gameState() {
			// Expose the main game state
			return gameState;
		},
		get liveVoteFeed() {
			// Expose the live vote feed
			return liveVoteFeed;
		},
		actions
	};
}

export const dealNoDealStore = createDealNoDealStore();
