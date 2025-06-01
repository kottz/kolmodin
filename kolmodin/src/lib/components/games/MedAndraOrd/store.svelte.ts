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
	setGameTimeLimit: (minutes: number) => void;
	setPointLimitEnabled: (enabled: boolean) => void;
	setTimeLimitEnabled: (enabled: boolean) => void;
}

function createInitialMedAndraOrdState(): MedAndraOrdGameState {
	return {
		phase: { type: 'Setup' },
		target_points: 10,
		game_time_limit_minutes: 5,
		point_limit_enabled: true,
		time_limit_enabled: true,
		player_scores: {},
		round_duration_seconds: 60 // Fixed at 60 seconds per word
	};
}

function createMedAndraOrdStore() {
	const gameState = $state<MedAndraOrdGameState>(createInitialMedAndraOrdState());

	// Client-side timers
	let wordTimerInterval: number | null = null;
	let gameTimerInterval: number | null = null;
	let clientWordTimer = $state(60);
	let clientGameTimer = $state(300); // 5 minutes in seconds

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

	// Game end reason
	const gameEndReason = $derived(() => {
		if (gameState.phase.type === 'GameOver') {
			return gameState.phase.data.reason;
		}
		return null;
	});

	// Display timers
	const displayWordTimer = $derived(() => {
		return gameState.phase.type === 'Playing' ? clientWordTimer : 60;
	});

	const displayGameTimer = $derived(() => {
		return gameState.phase.type === 'Playing'
			? clientGameTimer
			: gameState.game_time_limit_minutes * 60;
	});

	function startClientWordTimer() {
		stopClientWordTimer();
		clientWordTimer = 60; // Always 60 seconds per word

		wordTimerInterval = setInterval(() => {
			clientWordTimer--;
			if (clientWordTimer <= 0) {
				stopClientWordTimer();
				info('MedAndraOrdStore: Client word timer expired');
			}
		}, 1000);
	}

	function stopClientWordTimer() {
		if (wordTimerInterval) {
			clearInterval(wordTimerInterval);
			wordTimerInterval = null;
		}
	}

	function startClientGameTimer() {
		stopClientGameTimer();
		clientGameTimer = gameState.game_time_limit_minutes * 60;

		gameTimerInterval = setInterval(() => {
			clientGameTimer--;
			if (clientGameTimer <= 0) {
				stopClientGameTimer();
				info('MedAndraOrdStore: Client game timer expired');
			}
		}, 1000);
	}

	function stopClientGameTimer() {
		if (gameTimerInterval) {
			clearInterval(gameTimerInterval);
			gameTimerInterval = null;
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
				// Restart word timer but keep game timer running
				startClientWordTimer();
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

				// Start client timers only when first entering Playing phase
				if (previousPhase !== 'Playing' && eventPayload.data.new_phase.type === 'Playing') {
					startClientWordTimer();
					startClientGameTimer();
				} else if (eventPayload.data.new_phase.type !== 'Playing') {
					stopClientWordTimer();
					stopClientGameTimer();
				}
				break;

			case 'GameTimeUpdate':
				clientGameTimer = eventPayload.data.seconds_remaining;
				break;

			default:
				warn(`MedAndraOrdStore: Unhandled event type: ${(eventPayload as any).event_type}`);
		}
	}

	function sendCommand(
		command: MedAndraOrdCommandData['command'],
		points?: number,
		minutes?: number,
		enabled?: boolean
	): void {
		const commandData: MedAndraOrdCommandData = { command, points, minutes, enabled };
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
		setGameTimeLimit: (minutes: number) => sendCommand('SetGameTimeLimit', undefined, minutes),
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
		get gameEndReason() {
			return gameEndReason;
		},
		get displayWordTimer() {
			return displayWordTimer;
		},
		get displayGameTimer() {
			return displayGameTimer;
		},
		actions
	};
}

export const medAndraOrdStore = createMedAndraOrdStore();
