import { registerGameStore } from '$lib/services/game.event.router';
import { websocketStore } from '$lib/stores/websocket.store.svelte';
import type { ClientToServerMessage, GameSpecificCommandPayload } from '$lib/types/websocket.types';
import type {
	DealNoDealGameState,
	GameEventData,
	DealNoDealCommandData,
	DealNoDealPublicState
} from './types';
import type { StreamEvent } from '$lib/types/stream.types';
import { StreamEventManager } from '$lib/utils/stream.utils';
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

	// Stream event manager for broadcasting events
	const streamEventManager = new StreamEventManager(15);

	info('DealNoDealStore: Initializing store...');

	// Implement StreamableGameStore interface
	function getPublicState(): DealNoDealPublicState {
		// Create safe briefcase representation (no values for unopened cases)
		const publicBriefcases = gameState.briefcase_values.map((value, index) => ({
			index,
			isOpened: gameState.briefcase_is_opened[index] || false,
			...(gameState.briefcase_is_opened[index] && { value }) // Only include value if opened
		}));

		// Get current round info if in round opening phase
		let currentRoundInfo;
		if (gameState.phase.type === 'RoundCaseOpening_Voting') {
			currentRoundInfo = {
				roundNumber: gameState.phase.data.round_number,
				casesToOpen: gameState.phase.data.total_to_open_for_round,
				casesOpened: gameState.phase.data.opened_so_far_for_round
			};
		}

		// Get vote counts based on current phase
		let voteCounts;
		if (gameState.phase.type === 'DealOrNoDeal_Voting') {
			voteCounts = {
				DEAL: dealNoDealVotesMap.DEAL.length,
				'NO DEAL': dealNoDealVotesMap['NO DEAL'].length
			};
		} else if (gameState.phase.type === 'SwitchOrKeep_Voting') {
			voteCounts = {
				SWITCH: switchKeepVotesMap.SWITCH.length,
				KEEP: switchKeepVotesMap.KEEP.length
			};
		}

		// Create a clean, serializable copy of the phase to avoid DataCloneError
		const cleanPhase = {
			type: gameState.phase.type,
			...(gameState.phase.data && { data: JSON.parse(JSON.stringify(gameState.phase.data)) })
		};

		return {
			phase: cleanPhase,
			briefcases: publicBriefcases,
			playerChosenCaseIndex: gameState.player_chosen_case_index,
			remainingMoneyValues: [...(gameState.remaining_money_values_in_play || [])],
			currentRoundInfo,
			voteCounts,
			totalCases: gameState.briefcase_values.length
		};
	}

	function getStreamEvents(): StreamEvent[] {
		return streamEventManager.getEvents();
	}

	function clearStreamEvents(): void {
		streamEventManager.clearEvents();
	}

	function shouldBroadcastUpdate(): boolean {
		// Always broadcast updates for this game
		return true;
	}

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

				// Add stream event for voting
				streamEventManager.addEvent(
					'VOTE_CAST',
					{
						voter: voterUsername,
						vote: dealVote,
						message: `${voterUsername} voted ${dealVote}!`
					},
					2000
				);

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

				// Add stream event for voting
				streamEventManager.addEvent(
					'VOTE_CAST',
					{
						voter: voterUsername,
						vote: switchKeepVote,
						message: `${voterUsername} voted ${switchKeepVote}!`
					},
					2000
				);

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

		// Add stream event for case selection
		streamEventManager.addEvent(
			'CASE_SELECTED',
			{
				voter: voterUsername,
				caseNumber: newVotedCaseNumber,
				message: `${voterUsername} voted for Case #${newVotedCaseNumber}`
			},
			2000
		);

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
			case 'FullStateUpdate': {
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

				// Add phase change stream event
				const phaseType = eventPayload.data.phase.type;
				let phaseMessage = '';
				switch (phaseType) {
					case 'PlayerCaseSelection_Voting':
						phaseMessage = '🎯 Choose your lucky case!';
						break;
					case 'RoundCaseOpening_Voting':
						phaseMessage = '📦 Opening cases this round...';
						break;
					case 'BankerOfferCalculation':
						phaseMessage = '🏦 Banker is calculating offer...';
						break;
					case 'DealOrNoDeal_Voting':
						phaseMessage = '🤝 Deal or No Deal decision time!';
						break;
					case 'SwitchOrKeep_Voting':
						phaseMessage = '🔄 Final decision: Switch or Keep?';
						break;
					case 'GameOver':
						phaseMessage = '💰 Final reveal!';
						break;
				}

				if (phaseMessage) {
					streamEventManager.addEvent(
						'PHASE_CHANGED',
						{ phase: phaseType, message: phaseMessage },
						3000
					);
				}
				break;
			}
			case 'PlayerVoteRegistered': {
				info('DealNoDealStore: PlayerVoteRegistered event received:', eventPayload.data);
				updateLocalVoteDisplay(eventPayload.data.voter_username, eventPayload.data.vote_value);
				break;
			}
			case 'CaseOpened': {
				info(
					`DealNoDealStore: CaseOpened event received for case ${eventPayload.data.case_index + 1} with value ${eventPayload.data.value}`
				);

				// Add dramatic case opening stream event
				streamEventManager.addEvent(
					'CASE_OPENED',
					{
						caseNumber: eventPayload.data.case_index + 1,
						value: eventPayload.data.value,
						isPlayerCase: eventPayload.data.is_player_case_reveal_at_end,
						message: `📦 Case #${eventPayload.data.case_index + 1} revealed: $${eventPayload.data.value.toLocaleString()}!`
					},
					4000
				);
				break;
			}
			case 'BankerOfferPresented': {
				info(
					`DealNoDealStore: BankerOfferPresented event received: ${eventPayload.data.offer_amount}`
				);

				// Add banker offer stream event
				streamEventManager.addEvent(
					'BANKER_OFFER',
					{
						amount: eventPayload.data.offer_amount,
						message: `🏦 Banker offers: $${eventPayload.data.offer_amount.toLocaleString()}!`
					},
					5000
				);
				break;
			}
			default: {
				warn(`DealNoDealStore: Unhandled event type received`);
			}
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
		startGame: () => {
			sendCommand('StartGame');
			// Add game start stream event
			streamEventManager.addEvent(
				'GAME_STARTED',
				{
					message: '🎮 Deal or No Deal has started!'
				},
				3000
			);
		},
		concludeVotingAndProcess: () => sendCommand('ConcludeVotingAndProcess')
	};

	// Register with game event router, including streaming capabilities
	registerGameStore(GAME_TYPE_ID, {
		processEvent,
		getPublicState,
		getStreamEvents,
		clearStreamEvents,
		shouldBroadcastUpdate
	});

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

		// Streaming interface
		getPublicState,
		getStreamEvents,
		clearStreamEvents,
		shouldBroadcastUpdate,

		actions
	};
}

export const dealNoDealStore = createDealNoDealStore();
