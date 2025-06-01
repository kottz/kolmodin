import { registerGameStore } from '$lib/services/game.event.router';
import { websocketStore } from '$lib/stores/websocket.store.svelte';
import type { ClientToServerMessage, GameSpecificCommandPayload } from '$lib/types/websocket.types';
import type { MedAndraOrdGameState, GameEventData, MedAndraOrdCommandData } from './types';
import { debug, warn, info } from '$lib/utils/logger';

const GAME_TYPE_ID = 'MedAndraOrd'; // Must match Rust GAME_TYPE_ID

interface MedAndraOrdStoreActions {
	startGame: () => void;
	passWord: () => void;
	resetGame: () => void;
	setTargetPoints: (points: number) => void;
	setRoundDuration: (seconds: number) => void;
	setPointLimitEnabled: (enabled: boolean) => void;
	setTimeLimitEnabled: (enabled: boolean) => void;
}

function createInitialMedAndraOrdState(): MedAndraOrdGameState {
	return {
		phase: { type: 'Setup' },
		target_points: 10,
		round_duration_seconds: 60,
		point_limit_enabled: true,
		time_limit_enabled: false,
		player_scores: {}
	};
}

function createMedAndraOrdStore() {
	const gameState = $state<MedAndraOrdGameState>(createInitialMedAndraOrdState());

	// Client-side timer
	let timerInterval: number | null = null;
	let clientTimer = $state(60);

	// Derived state for leaderboard (sorted by points descending)
	const leaderboard = $derived(() => {
		return Object.entries(gameState.player_scores)
			.map(([player, points]) => ({ player, points }))
			.sort((a, b) => b.points - a.points);
	});

	// Current word (only visible during Playing phase)
	const currentWord = $derived(() => {
		if (gameState.phase.type === 'Playing') {
			return gameState.phase.data.current_word;
		}
		return null;
	});

	// Winner (only during GameOver phase)
	const winner = $derived(() => {
		if (gameState.phase.type === 'GameOver') {
			return gameState.phase.data.winner;
		}
		return null;
	});

	// Display timer
	const displayTimer = $derived(() => {
		return gameState.phase.type === 'Playing' ? clientTimer : gameState.round_duration_seconds;
	});

	function startClientTimer() {
		stopClientTimer();
		clientTimer = gameState.round_duration_seconds;

		timerInterval = setInterval(() => {
			clientTimer--;
			if (clientTimer <= 0) {
				stopClientTimer();
				info('MedAndraOrdStore: Client timer expired');
			}
		}, 1000);
	}

	function stopClientTimer() {
		if (timerInterval) {
			clearInterval(timerInterval);
			timerInterval = null;
		}
	}

	function processEvent(eventPayload: GameEventData): void {
		debug(`MedAndraOrdStore: Processing event "${eventPayload.event_type}"`);

		switch (eventPayload.event_type) {
			case 'FullStateUpdate':
				Object.assign(gameState, eventPayload.data);
				info('MedAndraOrdStore: Full state updated.');
				break;

			case 'WordChanged':
				info(`MedAndraOrdStore: Word changed to: ${eventPayload.data.word}`);
				// Restart timer for new word
				startClientTimer();
				break;

			case 'PlayerScored':
				info(
					`MedAndraOrdStore: ${eventPayload.data.player} scored! Points: ${eventPayload.data.points}`
				);
				gameState.player_scores[eventPayload.data.player] = eventPayload.data.points;
				break;

			case 'GamePhaseChanged':
				info(`MedAndraOrdStore: Game phase changed to: ${eventPayload.data.new_phase.type}`);
				const previousPhase = gameState.phase.type;
				gameState.phase = eventPayload.data.new_phase;

				// Start client timer only when first entering Playing phase
				if (previousPhase !== 'Playing' && eventPayload.data.new_phase.type === 'Playing') {
					startClientTimer();
				} else if (eventPayload.data.new_phase.type !== 'Playing') {
					stopClientTimer();
				}
				break;

			default:
				warn(`MedAndraOrdStore: Unhandled event type: ${(eventPayload as any).event_type}`);
		}
	}

	function sendCommand(
		command: MedAndraOrdCommandData['command'],
		points?: number,
		seconds?: number,
		enabled?: boolean
	): void {
		const commandData: MedAndraOrdCommandData = { command, points, seconds, enabled };
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

	const actions: MedAndraOrdStoreActions = {
		startGame: () => sendCommand('StartGame'),
		passWord: () => sendCommand('PassWord'),
		resetGame: () => sendCommand('ResetGame'),
		setTargetPoints: (points: number) => sendCommand('SetTargetPoints', points),
		setRoundDuration: (seconds: number) => sendCommand('SetRoundDuration', undefined, seconds),
		setPointLimitEnabled: (enabled: boolean) =>
			sendCommand('SetPointLimitEnabled', undefined, undefined, enabled),
		setTimeLimitEnabled: (enabled: boolean) =>
			sendCommand('SetTimeLimitEnabled', undefined, undefined, enabled)
	};

	// Register with game event router
	registerGameStore(GAME_TYPE_ID, { processEvent });

	return {
		get gameState() {
			return gameState;
		},
		get leaderboard() {
			return leaderboard;
		},
		get currentWord() {
			return currentWord;
		},
		get winner() {
			return winner;
		},
		get displayTimer() {
			return displayTimer;
		},
		actions
	};
}

export const medAndraOrdStore = createMedAndraOrdStore();
