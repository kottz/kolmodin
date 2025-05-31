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

// Updated type for deal/no deal votes
type DealNoDealVoteOption = 'DEAL' | 'NO DEAL';
type DealNoDealVotesMapType = {
	DEAL: string[]; // Array of usernames who voted DEAL
	'NO DEAL': string[]; // Array of usernames who voted NO DEAL
};

type SwitchKeepVoteOption = 'SWITCH' | 'KEEP';
type SwitchKeepVotesMapType = {
	SWITCH: string[]; // Array of usernames who voted SWITCH
	KEEP: string[]; // Array of usernames who voted KEEP
};

function createDealNoDealStore() {
	const gameState = $state<DealNoDealGameState>(createInitialDndState());
	const caseVotesMap = $state<CaseVotesMapType>({});
	const playerVoteRecord = $state<PlayerVoteRecordType>({});
	// Initialize with empty arrays for DEAL and NO DEAL
	const dealNoDealVotesMap = $state<DealNoDealVotesMapType>({ DEAL: [], 'NO DEAL': [] });
	// Add switch/keep votes map
	const switchKeepVotesMap = $state<SwitchKeepVotesMapType>({ SWITCH: [], KEEP: [] });

	info('DealNoDealStore: Initializing store...');

	function removeUserFromSwitchKeepLists(username: string) {
		let changed = false;
		const switchIndex = switchKeepVotesMap['SWITCH'].indexOf(username);
		if (switchIndex > -1) {
			switchKeepVotesMap['SWITCH'].splice(switchIndex, 1);
			changed = true;
		}
		const keepIndex = switchKeepVotesMap['KEEP'].indexOf(username);
		if (keepIndex > -1) {
			switchKeepVotesMap['KEEP'].splice(keepIndex, 1);
			changed = true;
		}
		return changed;
	}

	function removeUserFromDealNoDealLists(username: string) {
		let changed = false;
		const dealIndex = dealNoDealVotesMap['DEAL'].indexOf(username);
		if (dealIndex > -1) {
			dealNoDealVotesMap['DEAL'].splice(dealIndex, 1);
			changed = true;
		}
		const noDealIndex = dealNoDealVotesMap['NO DEAL'].indexOf(username);
		if (noDealIndex > -1) {
			dealNoDealVotesMap['NO DEAL'].splice(noDealIndex, 1);
			changed = true;
		}
		// Also clear switch/keep votes when clearing deal/no deal
		if (removeUserFromSwitchKeepLists(username)) {
			changed = true;
		}
		return changed;
	}

	function updateLocalVoteDisplay(voterUsername: string, voteValue: string) {
		const newVotedCaseNumber = parseInt(voteValue, 10);

		if (isNaN(newVotedCaseNumber)) {
			// This vote is NOT for a case number (e.g., "DEAL", "NO DEAL", "SWITCH", "KEEP", etc.).

			// 1. Clear player's previous case vote (if any).
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
				delete playerVoteRecord[voterUsername];
				info(
					`DealNoDealStore: Cleared previous case vote for ${voterUsername} due to non-case vote "${voteValue}".`
				);
			}

			// 2. Handle "DEAL" / "NO DEAL" vote.
			if (voteValue === 'DEAL' || voteValue === 'NO DEAL') {
				const dealVote = voteValue as DealNoDealVoteOption;
				const otherVote: DealNoDealVoteOption = dealVote === 'DEAL' ? 'NO DEAL' : 'DEAL';

				// Clear any switch/keep votes for this user
				removeUserFromSwitchKeepLists(voterUsername);

				// Remove from the other list (if present)
				const otherListIndex = dealNoDealVotesMap[otherVote].indexOf(voterUsername);
				if (otherListIndex > -1) {
					dealNoDealVotesMap[otherVote].splice(otherListIndex, 1);
				}

				// Add to the new list if not already present
				if (!dealNoDealVotesMap[dealVote].includes(voterUsername)) {
					dealNoDealVotesMap[dealVote].push(voterUsername);
				}
				info(
					`DealNoDealStore: dealNoDealVotesMap updated for vote by ${voterUsername}: ${dealVote}. Map:`,
					JSON.parse(JSON.stringify(dealNoDealVotesMap))
				);
			}
			// 3. Handle "SWITCH" / "KEEP" vote.
			else if (voteValue === 'SWITCH' || voteValue === 'KEEP') {
				const switchKeepVote = voteValue as SwitchKeepVoteOption;
				const otherVote: SwitchKeepVoteOption = switchKeepVote === 'SWITCH' ? 'KEEP' : 'SWITCH';

				// Clear any deal/no deal votes for this user
				removeUserFromDealNoDealLists(voterUsername);

				// Remove from the other list (if present)
				const otherListIndex = switchKeepVotesMap[otherVote].indexOf(voterUsername);
				if (otherListIndex > -1) {
					switchKeepVotesMap[otherVote].splice(otherListIndex, 1);
				}

				// Add to the new list if not already present
				if (!switchKeepVotesMap[switchKeepVote].includes(voterUsername)) {
					switchKeepVotesMap[switchKeepVote].push(voterUsername);
				}
				info(
					`DealNoDealStore: switchKeepVotesMap updated for vote by ${voterUsername}: ${switchKeepVote}. Map:`,
					JSON.parse(JSON.stringify(switchKeepVotesMap))
				);
			} else {
				// This is a non-case vote that isn't "DEAL", "NO DEAL", "SWITCH", or "KEEP".
				// Clear any existing votes for this player.
				if (removeUserFromDealNoDealLists(voterUsername)) {
					info(
						`DealNoDealStore: Cleared all votes for ${voterUsername} due to other non-case vote "${voteValue}".`
					);
				}
				warn(
					`DealNoDealStore: Received unhandled non-case vote type "${voteValue}" from ${voterUsername}. All votes cleared.`
				);
			}
			return;
		}

		// If we reach here, it IS a case vote.
		const newVotedCaseIndex = newVotedCaseNumber - 1;

		// 1. Clear player's previous non-case votes (if any).
		if (removeUserFromDealNoDealLists(voterUsername)) {
			info(
				`DealNoDealStore: Cleared all non-case votes for ${voterUsername} due to new case vote for case ${newVotedCaseNumber}.`
			);
		}

		// 2. Update caseVotesMap and playerVoteRecord for the case vote.
		const oldVotedCaseIndex = playerVoteRecord[voterUsername];
		if (oldVotedCaseIndex !== undefined && caseVotesMap[oldVotedCaseIndex]) {
			if (oldVotedCaseIndex !== newVotedCaseIndex) {
				caseVotesMap[oldVotedCaseIndex] = caseVotesMap[oldVotedCaseIndex].filter(
					(name) => name !== voterUsername
				);
				if (caseVotesMap[oldVotedCaseIndex].length === 0) {
					delete caseVotesMap[oldVotedCaseIndex];
				}
			}
		}

		if (!caseVotesMap[newVotedCaseIndex]) {
			caseVotesMap[newVotedCaseIndex] = [];
		}
		if (!caseVotesMap[newVotedCaseIndex].includes(voterUsername)) {
			caseVotesMap[newVotedCaseIndex].push(voterUsername);
		}
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
				info(eventPayload.data);

				Object.keys(caseVotesMap).forEach((key) => delete caseVotesMap[Number(key)]);
				Object.keys(playerVoteRecord).forEach((key) => delete playerVoteRecord[key]);
				// Reset all vote maps to initial empty lists
				dealNoDealVotesMap['DEAL'] = [];
				dealNoDealVotesMap['NO DEAL'] = [];
				switchKeepVotesMap['SWITCH'] = [];
				switchKeepVotesMap['KEEP'] = [];
				info('DealNoDealStore: All vote maps cleared due to FullStateUpdate.');
				break;
			case 'PlayerVoteRegistered':
				info('DealNoDealStore: PlayerVoteRegistered event received:', eventPayload.data);
				updateLocalVoteDisplay(eventPayload.data.voter_username, eventPayload.data.vote_value);
				break;
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
		get dealNoDealVotesMap() {
			return dealNoDealVotesMap;
		},
		get switchKeepVotesMap() {
			return switchKeepVotesMap;
		},
		actions
	};
}

export const dealNoDealStore = createDealNoDealStore();
