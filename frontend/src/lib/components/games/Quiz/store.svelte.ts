// src/lib/components/games/Quiz/store.svelte.ts

import { registerGameStore, broadcastCurrentGameState } from '$lib/services/game.event.router';
import { websocketStore } from '$lib/stores/websocket.store.svelte';
import type { ClientToServerMessage, GameSpecificCommandPayload } from '$lib/types/websocket.types';
import type { QuizGameState, GameEventData, QuizCommandData, QuizPublicState } from './types';
import type { StreamEvent, BasePublicGameState } from '$lib/types/stream.types';
import { StreamEventManager, createPublicLeaderboard } from '$lib/utils/stream.utils';
import { debug, warn, info } from '$lib/utils/logger';

const GAME_TYPE_ID = 'Quiz'; // Must match Rust GAME_TYPE_ID

interface QuizStoreActions {
	startGame: () => void;
	passQuestion: () => void;
	resetGame: () => void;
	setTargetPoints: (points: number) => void;
	setGameDuration: (seconds: number) => void;
	setPointLimitEnabled: (enabled: boolean) => void;
	setTimeLimitEnabled: (enabled: boolean) => void;
	removeRecentGuess: (guessId: string) => void;
}

function createInitialQuizState(): QuizGameState {
	return {
		phase: { type: 'Setup' },
		target_points: 10,
		game_duration_seconds: 300, // 5 minutes default
		point_limit_enabled: true,
		time_limit_enabled: false,
		player_scores: {},
		recent_guesses: []
	};
}

function createQuizStore() {
	const gameState = $state<QuizGameState>(createInitialQuizState());
	const streamEventManager = new StreamEventManager(10);

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

	// Current question (only visible during Playing phase)
	const currentQuestion = $derived(() => {
		if (gameState.phase.type === 'Playing') {
			return gameState.phase.data.current_question;
		}
		return null;
	});

	// Current answer (only visible during Playing phase)
	const currentAnswer = $derived(() => {
		if (gameState.phase.type === 'Playing') {
			return gameState.phase.data.current_answer;
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

				// Add time warning events
				if (clientTimer === 30 || clientTimer === 10 || clientTimer === 5) {
					streamEventManager.addEvent(
						'TIME_WARNING',
						{ remainingSeconds: clientTimer, message: `${clientTimer} seconds remaining!` },
						2000
					);
				}

				if (clientTimer <= 0) {
					stopGameTimer();
					info('QuizStore: Game timer expired');
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

	// Implement StreamableGameStore interface
	function getPublicState(): QuizPublicState {
		const publicLeaderboard = createPublicLeaderboard(gameState.player_scores, 10);

		// Create a clean, serializable object
		const publicState: QuizPublicState = {
			phase: {
				type: gameState.phase.type,
				// Ensure data is serializable
				data:
					gameState.phase.type === 'Playing'
						? { hasQuestion: true, currentQuestion: gameState.phase.data.current_question }
						: gameState.phase.type === 'GameOver' && gameState.phase.data
							? { winner: gameState.phase.data.winner }
							: undefined
			},
			targetPoints: gameState.target_points,
			gameDurationSeconds: gameState.game_duration_seconds,
			pointLimitEnabled: gameState.point_limit_enabled,
			timeLimitEnabled: gameState.time_limit_enabled,
			leaderboard: publicLeaderboard,
			playersCount: Object.keys(gameState.player_scores).length
		};

		// Add timeRemaining only if applicable
		if (gameState.phase.type === 'Playing' && gameState.time_limit_enabled) {
			publicState.timeRemaining = clientTimer;
		}

		// Ensure it's serializable by doing a test JSON parse/stringify
		try {
			JSON.parse(JSON.stringify(publicState));
			return publicState;
		} catch (error) {
			console.error('QuizStore: Public state is not serializable:', error);
			// Return a minimal safe state
			return {
				phase: { type: gameState.phase.type },
				targetPoints: gameState.target_points,
				gameDurationSeconds: gameState.game_duration_seconds,
				pointLimitEnabled: gameState.point_limit_enabled,
				timeLimitEnabled: gameState.time_limit_enabled,
				leaderboard: [],
				playersCount: 0
			};
		}
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

	function processEvent(eventPayload: GameEventData): void {
		debug(`QuizStore: Processing event "${eventPayload.event_type}"`);

		switch (eventPayload.event_type) {
			case 'FullStateUpdate':
				Object.assign(gameState, eventPayload.data);
				info('QuizStore: Full state updated.');
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
				// Broadcast state update to stream window
				broadcastCurrentGameState();
				break;

			case 'QuestionChanged':
				info(`QuizStore: Question changed to: ${eventPayload.data.question}`);
				// Add stream event for question change
				streamEventManager.addEvent(
					'QUESTION_CHANGED',
					{ message: 'New question available!' },
					2000
				);
				break;

			case 'PlayerScored': {
				info(`QuizStore: ${eventPayload.data.player} scored! Points: ${eventPayload.data.points}`);
				gameState.player_scores[eventPayload.data.player] = eventPayload.data.points;

				// Add celebration stream event with current answer
				const currentCorrectAnswer =
					gameState.phase.type === 'Playing' ? gameState.phase.data.current_answer : '';
				streamEventManager.addEvent(
					'CORRECT_ANSWER',
					{
						player: eventPayload.data.player,
						answer: currentCorrectAnswer,
						points: eventPayload.data.points,
						message: `🎉 ${eventPayload.data.player} was correct! 🎉`
					},
					4000
				);

				// Broadcast state update to stream window (for leaderboard updates)
				broadcastCurrentGameState();
				break;
			}

			case 'GamePhaseChanged': {
				info(`QuizStore: Game phase changed to: ${eventPayload.data.new_phase.type}`);
				const previousPhase = gameState.phase.type;
				gameState.phase = eventPayload.data.new_phase;

				// Add phase change stream event
				let phaseMessage = '';
				switch (eventPayload.data.new_phase.type) {
					case 'Playing':
						phaseMessage = 'Quiz Started! Start answering!';
						streamEventManager.addEvent('GAME_STARTED', { message: phaseMessage }, 3000);
						break;
					case 'GameOver': {
						const winnerName = eventPayload.data.new_phase.data?.winner;
						phaseMessage = winnerName ? `🏆 ${winnerName} Wins! 🏆` : 'Quiz Over!';
						streamEventManager.addEvent(
							'GAME_ENDED',
							{
								winner: winnerName,
								message: phaseMessage
							},
							5000
						);
						break;
					}
					case 'Setup':
						phaseMessage = 'Setting up new quiz...';
						streamEventManager.addEvent('GAME_RESET', { message: phaseMessage }, 2000);
						break;
				}

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

				// Broadcast state update to stream window (critical for game over detection)
				console.log(
					'Broadcasting game phase change to stream window, new phase:',
					eventPayload.data.new_phase.type
				);
				broadcastCurrentGameState();
				break;
			}

			case 'GameTimeUpdate':
				// Handle server time updates if implemented
				if (eventPayload.data && 'remaining_seconds' in eventPayload.data) {
					clientTimer = eventPayload.data.remaining_seconds;
				}
				break;

			case 'RecentGuessesUpdated':
				info('QuizStore: Recent guesses updated');
				gameState.recent_guesses = eventPayload.data.recent_guesses;

				// Add stream event for recent guesses update
				streamEventManager.addEvent(
					'RECENT_GUESSES_UPDATED',
					{ recentGuesses: eventPayload.data.recent_guesses },
					1000
				);

				// Broadcast state update to stream window
				broadcastCurrentGameState();
				break;

			default:
				warn(
					`QuizStore: Unhandled event type: ${(eventPayload as { event_type: string }).event_type}`
				);
		}
	}

	function sendCommand(
		command: QuizCommandData['command'],
		points?: number,
		seconds?: number,
		enabled?: boolean,
		guess_id?: string
	): void {
		const commandData: QuizCommandData = { command, points, seconds, enabled, guess_id };
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

	const actions: QuizStoreActions = {
		startGame: () => sendCommand('StartGame'),
		passQuestion: () => sendCommand('PassQuestion'),
		resetGame: () => sendCommand('ResetGame'),
		setTargetPoints: (points: number) => sendCommand('SetTargetPoints', points),
		setGameDuration: (seconds: number) => sendCommand('SetGameDuration', undefined, seconds),
		setPointLimitEnabled: (enabled: boolean) =>
			sendCommand('SetPointLimitEnabled', undefined, undefined, enabled),
		setTimeLimitEnabled: (enabled: boolean) =>
			sendCommand('SetTimeLimitEnabled', undefined, undefined, enabled),
		removeRecentGuess: (guessId: string) =>
			sendCommand('RemoveRecentGuess', undefined, undefined, undefined, guessId)
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
		get leaderboard() {
			return leaderboard;
		},
		get currentQuestion() {
			return currentQuestion;
		},
		get currentAnswer() {
			return currentAnswer;
		},
		get winner() {
			return winner;
		},
		get displayTimer() {
			return displayTimer;
		},

		// Streaming interface
		getPublicState,
		getStreamEvents,
		clearStreamEvents,
		shouldBroadcastUpdate,

		actions
	};
}

export const quizStore = createQuizStore();
