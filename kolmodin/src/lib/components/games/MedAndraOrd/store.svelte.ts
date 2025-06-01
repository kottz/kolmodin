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
	setGameDuration: (seconds: number) => void;
	setPointLimitEnabled: (enabled: boolean) => void;
	setTimeLimitEnabled: (enabled: boolean) => void;
}

function createInitialMedAndraOrdState(): MedAndraOrdGameState {
	return {
		phase: { type: 'Setup' },
		target_points: 10,
		game_duration_seconds: 300, // 5 minutes default
		point_limit_enabled: true,
		time_limit_enabled: false,
		player_scores: {}
	};
}

function createMedAndraOrdStore() {
	const gameState = $state<MedAndraOrdGameState>(createInitialMedAndraOrdState());

	// Client-side game timer (counts down total game time)
	let timerInterval: number | null = null;
	let clientTimer = $state(300); // Default 5 minutes
	let gameTimerStarted = $state(false); // Track if timer has been started for this game

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

	// Display timer - shows game time remaining or total game duration
	const displayTimer = $derived(() => {
		return gameState.phase.type === 'Playing' && gameState.time_limit_enabled
			? clientTimer
			: gameState.game_duration_seconds;
	});

	function startGameTimer() {
		// Only start timer if it hasn't been started for this game session
		if (gameTimerStarted) {
			return;
		}

		stopGameTimer();
		clientTimer = gameState.game_duration_seconds;
		gameTimerStarted = true;

		if (gameState.time_limit_enabled) {
			timerInterval = setInterval(() => {
				clientTimer--;
				if (clientTimer <= 0) {
					stopGameTimer();
					info('MedAndraOrdStore: Game timer expired');
					// Note: Server should handle game end, but we stop the timer here
				}
			}, 1000);
		}
	}

	function stopGameTimer() {
		if (timerInterval) {
			clearInterval(timerInterval);
			timerInterval = null;
		}
	}

	function resetGameTimer() {
		stopGameTimer();
		gameTimerStarted = false;
		clientTimer = gameState.game_duration_seconds;
	}

	function processEvent(eventPayload: GameEventData): void {
		debug(`MedAndraOrdStore: Processing event "${eventPayload.event_type}"`);

		switch (eventPayload.event_type) {
			case 'FullStateUpdate':
				Object.assign(gameState, eventPayload.data);
				info('MedAndraOrdStore: Full state updated.');
				// If game is in setup, reset timer flag
				if (gameState.phase.type === 'Setup') {
					resetGameTimer();
				}
				// Update client timer to match server state if game is playing
				else if (
					gameState.phase.type === 'Playing' &&
					gameState.time_limit_enabled &&
					!gameTimerStarted
				) {
					startGameTimer();
				}
				break;

			case 'WordChanged':
				info(`MedAndraOrdStore: Word changed to: ${eventPayload.data.word}`);
				// No timer reset - timer continues counting down
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

				// Only start timer when transitioning from Setup to Playing
				if (previousPhase === 'Setup' && eventPayload.data.new_phase.type === 'Playing') {
					startGameTimer();
				}
				// Reset timer when going back to Setup or ending game
				else if (
					eventPayload.data.new_phase.type === 'Setup' ||
					eventPayload.data.new_phase.type === 'GameOver'
				) {
					resetGameTimer();
				}
				break;

			case 'GameTimeUpdate':
				// Handle server time updates if implemented
				if (eventPayload.data && 'remaining_seconds' in eventPayload.data) {
					clientTimer = eventPayload.data.remaining_seconds;
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
		setGameDuration: (seconds: number) => sendCommand('SetGameDuration', undefined, seconds),
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
