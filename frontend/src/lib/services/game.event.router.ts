// src/lib/services/game.event.router.ts

import type { GameSpecificEventPayload } from '$lib/types/websocket.types';
import type { StreamableGameStore } from '$lib/types/stream.types';
import { broadcastService } from './broadcast.service';
import { warn, debug, info } from '$lib/utils/logger';

// Define an interface for what a game-specific store's event processor should look like.
// Enhanced to include streaming capabilities
interface GameStoreEventProcessor extends Partial<StreamableGameStore> {
	processEvent: (eventData: unknown) => void; // `eventData` will be specific to the game
}

// This registry will map game_type_id to their respective store's event processor.
const gameStoreRegistry = new Map<string, GameStoreEventProcessor>();

// Track the currently active game for broadcasting
let currentActiveGame: string | null = null;

// Function to register a game store's event processor.
// Game-specific stores will call this when they are initialized.
export function registerGameStore(gameTypeId: string, store: GameStoreEventProcessor): void {
	if (gameStoreRegistry.has(gameTypeId)) {
		warn(
			`Game Event Router: A store for game type ID "${gameTypeId}" is already registered. Overwriting.`
		);
	}
	gameStoreRegistry.set(gameTypeId, store);
	debug(`Game Event Router: Registered store for game type ID "${gameTypeId}".`);
}

// Function to unregister a game store
export function unregisterGameStore(gameTypeId: string): void {
	if (gameStoreRegistry.has(gameTypeId)) {
		gameStoreRegistry.delete(gameTypeId);
		debug(`Game Event Router: Unregistered store for game type ID "${gameTypeId}".`);
	}
}

// Set the currently active game (called by UI when game becomes active)
export function setActiveGame(gameTypeId: string | null): void {
	if (currentActiveGame !== gameTypeId) {
		const previousGame = currentActiveGame;
		currentActiveGame = gameTypeId;

		info(`Game Event Router: Active game changed from ${previousGame} to ${gameTypeId}`);

		// Initialize broadcast service if not already done (admin window)
		if (!broadcastService.isInitialized() && !broadcastService.getIsStreamWindow()) {
			broadcastService.initialize(false); // false = admin window
		}

		// Broadcast game change
		broadcastService.broadcastGameChanged(gameTypeId);

		// If switching to a new game, broadcast initial state
		if (gameTypeId && gameStoreRegistry.has(gameTypeId)) {
			const store = gameStoreRegistry.get(gameTypeId)!;
			if (store.getPublicState) {
				const publicState = store.getPublicState();
				broadcastService.broadcastStateUpdate(gameTypeId, publicState);
			}
		}
	}
}

// Get the currently active game
export function getActiveGame(): string | null {
	return currentActiveGame;
}

// Broadcast state update for the currently active game
export function broadcastCurrentGameState(): void {
	if (!currentActiveGame) {
		console.log('Game Event Router: No active game to broadcast state for');
		debug('Game Event Router: No active game to broadcast state for');
		return;
	}

	const store = gameStoreRegistry.get(currentActiveGame);
	if (store && store.getPublicState) {
		if (!store.shouldBroadcastUpdate || store.shouldBroadcastUpdate()) {
			const publicState = store.getPublicState();
			console.log(
				'Game Event Router: About to broadcast state for',
				currentActiveGame,
				'phase:',
				publicState?.phase?.type
			);
			console.log('BroadcastService initialized?', broadcastService.isInitialized());
			broadcastService.broadcastStateUpdate(currentActiveGame, publicState);
			debug(`Game Event Router: Broadcasted state update for ${currentActiveGame}`);
		} else {
			console.log('Game Event Router: Store says not to broadcast update');
		}
	} else {
		console.log(
			'Game Event Router: No store found or no getPublicState method for',
			currentActiveGame
		);
	}
}

// Broadcast stream events for the currently active game
export function broadcastCurrentGameEvents(): void {
	if (!currentActiveGame) {
		return;
	}

	const store = gameStoreRegistry.get(currentActiveGame);
	if (store && store.getStreamEvents) {
		const events = store.getStreamEvents();
		events.forEach((event) => {
			broadcastService.broadcastStreamEvent(currentActiveGame!, event);
		});

		// Clear events after broadcasting
		if (store.clearStreamEvents) {
			store.clearStreamEvents();
		}
	}
}

function routeGameSpecificEvent(payload: GameSpecificEventPayload): void {
	const { game_type_id, event_data } = payload;
	debug(`Game Event Router: Routing event for game type ID "${game_type_id}".`, event_data);

	const targetStore = gameStoreRegistry.get(game_type_id);

	if (targetStore && typeof targetStore.processEvent === 'function') {
		try {
			targetStore.processEvent(event_data);

			// After processing the event, check if we should broadcast updates
			if (game_type_id === currentActiveGame) {
				// Small delay to ensure state updates are complete
				setTimeout(() => {
					broadcastCurrentGameState();
					broadcastCurrentGameEvents();
				}, 0);
			}
		} catch (e) {
			warn(
				`Game Event Router: Error processing event in store for game type "${game_type_id}":`,
				e
			);
		}
	} else {
		warn(
			`Game Event Router: No registered store or invalid processEvent method for game type ID "${game_type_id}". Event not routed.`
		);
	}
}

// Cleanup function
export function cleanup(): void {
	currentActiveGame = null;
	broadcastService.cleanup();
	info('Game Event Router: Cleaned up');
}

export const gameEventRouter = {
	routeGameSpecificEvent,
	setActiveGame,
	getActiveGame,
	broadcastCurrentGameState,
	broadcastCurrentGameEvents,
	cleanup
};
