import { registerGameStore } from '$lib/services/game.event.router';
import { websocketStore } from '$lib/stores/websocket.store.svelte';
import type { ClientToServerMessage, GameSpecificCommandPayload } from '$lib/types/websocket.types';
import type { DealNoDealGameState, GameEventData, DealNoDealCommandData } from './types';
import { debug, warn, info } from '$lib/utils/logger';

const GAME_TYPE_ID = 'DealNoDeal';

interface DealNoDealStoreActions {
	startGame: () => void;
	concludeVotingAndProcess: () => void;
}

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

type CaseVotesMapType = Record<number, string[]>; // caseIndex (0-based) -> string[] (voter_usernames)
type PlayerVoteRecordType = Record<string, number>; // voter_username -> voted_case_index (0-based)

function createDealNoDealStore() {
	const gameState = $state<DealNoDealGameState>(createInitialDndState());
	const caseVotesMap = $state<CaseVotesMapType>({});
	const playerVoteRecord = $state<PlayerVoteRecordType>({});

	info('DealNoDealStore: Initializing store...');

	/**
	 * Updates the local caseVotesMap based on a new vote.
	 * Assumes the server has validated the vote (phase, case existence, etc.).
	 * vote_value is expected to be a string representing a 1-based case number for case votes.
	 */
	function updateLocalVoteDisplay(voterUsername: string, voteValue: string) {
		// Check if the voteValue is for a case (i.e., a number)
		const newVotedCaseNumber = parseInt(voteValue, 10);

		if (isNaN(newVotedCaseNumber)) {
			// This vote is not for a case number (e.g., "DEAL", "NO_DEAL").
			// We only update caseVotesMap for case number votes.
			// If there was a previous case vote by this player, we should clear it.
			const oldVotedCaseIndex = playerVoteRecord[voterUsername];
			if (oldVotedCaseIndex !== undefined) {
				if (caseVotesMap[oldVotedCaseIndex]) {
					caseVotesMap[oldVotedCaseIndex] = caseVotesMap[oldVotedCaseIndex].filter(
						(name) => name !== voterUsername
					);
					if (caseVotesMap[oldVotedCaseIndex].length === 0) {
						delete caseVotesMap[oldVotedCaseIndex];
					}
				}
				delete playerVoteRecord[voterUsername]; // Player is no longer voting for a specific case
				info(
					`DealNoDealStore: Cleared previous case vote for ${voterUsername} due to non-case vote "${voteValue}".`
				);
			}
			return; // Do not process non-case votes in this map.
		}

		// Server should ensure newVotedCaseNumber is valid for the current game state.
		// We just convert to 0-based index.
		const newVotedCaseIndex = newVotedCaseNumber - 1;

		// 1. Remove player's old vote from the map (if any)
		const oldVotedCaseIndex = playerVoteRecord[voterUsername];
		if (oldVotedCaseIndex !== undefined && caseVotesMap[oldVotedCaseIndex]) {
			// Ensure oldVotedCaseIndex is not the same as newVotedCaseIndex before filtering,
			// though the logic handles it, it's more explicit.
			if (oldVotedCaseIndex !== newVotedCaseIndex) {
				caseVotesMap[oldVotedCaseIndex] = caseVotesMap[oldVotedCaseIndex].filter(
					(name) => name !== voterUsername
				);
				if (caseVotesMap[oldVotedCaseIndex].length === 0) {
					delete caseVotesMap[oldVotedCaseIndex];
				}
			}
		}

		// 2. Add player's new vote to the map for the new case index
		if (!caseVotesMap[newVotedCaseIndex]) {
			caseVotesMap[newVotedCaseIndex] = [];
		}
		// Only add if not already there (covers case where player re-votes for same case, after old removed)
		if (!caseVotesMap[newVotedCaseIndex].includes(voterUsername)) {
			caseVotesMap[newVotedCaseIndex].push(voterUsername);
		}

		// 3. Update player's vote record
		playerVoteRecord[voterUsername] = newVotedCaseIndex;

		info(
			`DealNoDealStore: caseVotesMap updated for vote by ${voterUsername} for case index ${newVotedCaseIndex}. Map:`,
			JSON.parse(JSON.stringify(caseVotesMap))
		);
	}

	function processEvent(eventPayload: GameEventData): void {
		debug(
			`DealNoDealStore: Processing event type "${eventPayload.event_type}"`,
			JSON.parse(JSON.stringify(eventPayload.data))
		);
		switch (eventPayload.event_type) {
			case 'FullStateUpdate':
				Object.assign(gameState, eventPayload.data);
				info('DealNoDealStore: Full state updated.');
				// Clear local vote display maps as FullStateUpdate contains the official tally
				// and signals a new voting segment or phase.
				Object.keys(caseVotesMap).forEach((key) => delete caseVotesMap[Number(key)]);
				Object.keys(playerVoteRecord).forEach((key) => delete playerVoteRecord[key]);
				info('DealNoDealStore: caseVotesMap and playerVoteRecord cleared due to FullStateUpdate.');
				break;
			case 'PlayerVoteRegistered':
				info('DealNoDealStore: PlayerVoteRegistered event received:', eventPayload.data);
				// The server is the source of truth for phase and validity.
				// We only update the local display based on this event.
				updateLocalVoteDisplay(eventPayload.data.voter_username, eventPayload.data.vote_value);
				break;
			// ... other cases
			case 'CaseOpened':
				info(
					`DealNoDealStore: CaseOpened event received for case ${eventPayload.data.case_index + 1} with value ${eventPayload.data.value}`
				);
				break;
			case 'BankerOfferPresented':
				info(
					`DealNoDealStore: BankerOfferPresented event received: ${eventPayload.data.offer_amount}`
				);
				break;
			default:
				warn(`DealNoDealStore: Unhandled event type: ${(eventPayload as any).event_type}`);
		}
	}

	function sendCommand(command: DealNoDealCommandData['command']): void {
		// ... (sendCommand remains the same)
		const commandData: DealNoDealCommandData = { command };
		const payload: GameSpecificCommandPayload = {
			game_type_id: GAME_TYPE_ID,
			command_data: commandData
		};
		const messageToSend: ClientToServerMessage = {
			messageType: 'GameSpecificCommand',
			payload: payload
		};
		websocketStore.send(messageToSend);
	}

	const actions: DealNoDealStoreActions = {
		startGame: () => sendCommand('StartGame'),
		concludeVotingAndProcess: () => sendCommand('ConcludeVotingAndProcess')
	};

	registerGameStore(GAME_TYPE_ID, { processEvent });

	return {
		get gameState() {
			return gameState;
		},
		get caseVotesMap() {
			return caseVotesMap;
		},
		actions
	};
}

export const dealNoDealStore = createDealNoDealStore();
