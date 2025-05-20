import type { GameSpecificEventPayload } from '$lib/types/websocket.types';
import { warn, debug } from '$lib/utils/logger';

// Import game-specific stores here as they are created.
// For now, we'll use placeholders.
// Example: import { dealNoDealStore } from '$lib/components/games/DealNoDeal/store.svelte';
// Example: import { helloWorldStore } from '$lib/components/games/HelloWorldGame/store.svelte';

// Define an interface for what a game-specific store's event processor should look like.
// This helps ensure consistency.
interface GameStoreEventProcessor {
	processEvent: (eventData: any) => void; // `eventData` will be specific to the game
}

// This registry will map game_type_id to their respective store's event processor.
const gameStoreRegistry = new Map<string, GameStoreEventProcessor>();

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

// Function to unregister a game store (e.g., if a game module is dynamically unloaded, though less common in this setup).
export function unregisterGameStore(gameTypeId: string): void {
	if (gameStoreRegistry.has(gameTypeId)) {
		gameStoreRegistry.delete(gameTypeId);
		debug(`Game Event Router: Unregistered store for game type ID "${gameTypeId}".`);
	}
}

function routeGameSpecificEvent(payload: GameSpecificEventPayload): void {
	const { game_type_id, event_data } = payload;
	debug(`Game Event Router: Routing event for game type ID "${game_type_id}".`, event_data);

	const targetStore = gameStoreRegistry.get(game_type_id);

	if (targetStore && typeof targetStore.processEvent === 'function') {
		try {
			targetStore.processEvent(event_data);
		} catch (e) {
			warn(
				`Game Event Router: Error processing event in store for game type "${game_type_id}":`,
				e
			);
			// Optionally, notify the user or log more extensively
		}
	} else {
		warn(
			`Game Event Router: No registered store or invalid processEvent method for game type ID "${game_type_id}". Event not routed.`
		);
		// You might want to buffer these events or handle them differently if a game store
		// might register after events for it have already arrived. For now, we'll just warn.
	}
}

export const gameEventRouter = {
	routeGameSpecificEvent
	// Expose register/unregister if game stores are initialized dynamically outside this file,
	// otherwise, they can just call the exported registerGameStore function directly.
	// For simplicity with Svelte stores that auto-initialize on import, direct call is fine.
};

// --- How Game-Specific Stores Will Use This ---
// In each `src/lib/components/games/[GameTypeID]/store.svelte.ts`:
/*
import { registerGameStore } from '$lib/services/game.event.router';
import type { SomeGameEventData } from './types';

const GAME_TYPE_ID = 'SomeGame';

function createSomeGameStore() {
    // ... store state ...

    function processEvent(eventData: SomeGameEventData) {
        // ... process event and update state ...
        console.log(`Processing ${GAME_TYPE_ID} event:`, eventData);
    }

    // ... store actions ...

    // Register this store with the router when the store module is initialized
    registerGameStore(GAME_TYPE_ID, { processEvent });

    return {
        // ... exposed state and actions ...
    };
}

export const someGameStore = createSomeGameStore();
*/
