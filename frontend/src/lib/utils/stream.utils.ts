import type { StreamEvent, StreamableGameStore } from '$lib/types/stream.types';
import { debug } from '$lib/utils/logger';

// Helper class to manage stream events for game stores
export class StreamEventManager {
	private events: StreamEvent[] = [];
	private maxEvents: number;

	constructor(maxEvents: number = 10) {
		this.maxEvents = maxEvents;
	}

	addEvent(type: string, data: unknown, duration?: number): void {
		const event: StreamEvent = {
			type,
			data,
			duration,
			timestamp: Date.now()
		};

		this.events.push(event);

		// Keep only the most recent events
		if (this.events.length > this.maxEvents) {
			this.events = this.events.slice(-this.maxEvents);
		}

		debug(`StreamEventManager: Added event ${type}`, data);
	}

	getEvents(): StreamEvent[] {
		return [...this.events];
	}

	clearEvents(): void {
		this.events = [];
	}

	hasEvents(): boolean {
		return this.events.length > 0;
	}
}

// Helper function to create common stream events
export const createStreamEvent = {
	playerScored: (playerName: string, points: number, totalPoints?: number): StreamEvent => ({
		type: 'PLAYER_SCORED',
		data: {
			player: playerName,
			points,
			totalPoints,
			message: `${playerName} scored ${points} point${points !== 1 ? 's' : ''}!`
		},
		duration: 3000,
		timestamp: Date.now()
	}),

	gamePhaseChanged: (newPhase: string, description?: string): StreamEvent => ({
		type: 'PHASE_CHANGED',
		data: {
			phase: newPhase,
			description: description || `Game phase: ${newPhase}`
		},
		duration: 2000,
		timestamp: Date.now()
	}),

	playerJoined: (playerName: string): StreamEvent => ({
		type: 'PLAYER_JOINED',
		data: {
			player: playerName,
			message: `${playerName} joined the game`
		},
		duration: 2000,
		timestamp: Date.now()
	}),

	gameStarted: (gameType: string): StreamEvent => ({
		type: 'GAME_STARTED',
		data: {
			gameType,
			message: `${gameType} has started!`
		},
		duration: 3000,
		timestamp: Date.now()
	}),

	gameEnded: (winner?: string, summary?: string): StreamEvent => ({
		type: 'GAME_ENDED',
		data: {
			winner,
			summary: summary || (winner ? `${winner} wins!` : 'Game ended'),
			message: winner ? `🎉 ${winner} wins! 🎉` : 'Game Over'
		},
		duration: 5000,
		timestamp: Date.now()
	}),

	correctAnswer: (playerName: string, answer: string, word?: string): StreamEvent => ({
		type: 'CORRECT_ANSWER',
		data: {
			player: playerName,
			answer,
			word,
			message: `${playerName} got it right${word ? `: ${word}` : ''}!`
		},
		duration: 3000,
		timestamp: Date.now()
	}),

	timeWarning: (remainingSeconds: number): StreamEvent => ({
		type: 'TIME_WARNING',
		data: {
			remainingSeconds,
			message: `${remainingSeconds} seconds remaining!`
		},
		duration: 2000,
		timestamp: Date.now()
	}),

	achievement: (playerName: string, achievement: string): StreamEvent => ({
		type: 'ACHIEVEMENT',
		data: {
			player: playerName,
			achievement,
			message: `${playerName} earned: ${achievement}`
		},
		duration: 4000,
		timestamp: Date.now()
	})
};

// Utility function to filter sensitive data from game state
export function createPublicStateFilter<T extends Record<string, unknown>>(
	fullState: T,
	publicFields: (keyof T)[],
	transformations?: Partial<Record<keyof T, (value: unknown) => unknown>>
): Partial<T> {
	const publicState: Partial<T> = {};

	publicFields.forEach((field) => {
		if (field in fullState) {
			const value = fullState[field];
			const transform = transformations?.[field];
			publicState[field] = transform ? transform(value) : value;
		}
	});

	return publicState;
}

// Utility to safely extract leaderboard/scores from player data
export function createPublicLeaderboard(
	playerScores: Record<string, number>,
	limit?: number
): Array<{ player: string; points: number; rank: number }> {
	const leaderboard = Object.entries(playerScores)
		.map(([player, points]) => ({ player, points }))
		.sort((a, b) => b.points - a.points)
		.map((entry, index) => ({ ...entry, rank: index + 1 }));

	return limit ? leaderboard.slice(0, limit) : leaderboard;
}

// Mixin function to add streaming capabilities to existing game stores
export function addStreamingCapabilities<T extends object>(
	store: T,
	getPublicStateFn: () => unknown,
	shouldBroadcastFn?: () => boolean
): T &
	Pick<
		StreamableGameStore,
		'getPublicState' | 'getStreamEvents' | 'clearStreamEvents' | 'shouldBroadcastUpdate'
	> {
	const eventManager = new StreamEventManager();

	return {
		...store,
		getPublicState: getPublicStateFn,
		getStreamEvents: () => eventManager.getEvents(),
		clearStreamEvents: () => eventManager.clearEvents(),
		shouldBroadcastUpdate: shouldBroadcastFn || (() => true),
		// Add helper method to add stream events
		addStreamEvent: (type: string, data: unknown, duration?: number) => {
			eventManager.addEvent(type, data, duration);
		}
	} as T &
		Pick<
			StreamableGameStore,
			'getPublicState' | 'getStreamEvents' | 'clearStreamEvents' | 'shouldBroadcastUpdate'
		> & { addStreamEvent: (type: string, data: unknown, duration?: number) => void };
}
