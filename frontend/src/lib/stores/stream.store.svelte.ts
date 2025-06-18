// src/lib/stores/stream.store.svelte.ts

import { broadcastService } from '$lib/services/broadcast.service';
import { debug, info } from '$lib/utils/logger';
import type {
	BroadcastMessage,
	StreamEvent,
	StreamWindowState,
	StreamDisplayConfig,
	StateUpdateMessage,
	StreamEventMessage,
	GameChangedMessage,
	StreamControlMessage
} from '$lib/types/stream.types';

const DEFAULT_DISPLAY_CONFIG: StreamDisplayConfig = {
	showPlayerNames: true,
	showScores: true,
	showTimer: true,
	showPhase: true,
	animationDuration: 3000,
	maxActiveEvents: 3
};

function createStreamStore() {
	const state = $state<StreamWindowState>({
		isVisible: true,
		currentGameType: null,
		gameState: null,
		activeEvents: [],
		lastUpdateTimestamp: 0
	});

	const displayConfig = $state<StreamDisplayConfig>({ ...DEFAULT_DISPLAY_CONFIG });

	const eventCleanupTimeouts = new Map<string, number>();

	// Initialize the broadcast service for stream window
	function initialize(): void {
		if (!broadcastService.isInitialized()) {
			broadcastService.initialize(true); // true = isStreamWindow

			// Register message handlers
			broadcastService.registerMessageHandler('STATE_UPDATE', handleStateUpdate);
			broadcastService.registerMessageHandler('STREAM_EVENT', handleStreamEvent);
			broadcastService.registerMessageHandler('GAME_CHANGED', handleGameChanged);
			broadcastService.registerMessageHandler('STREAM_CONTROL', handleStreamControl);

			// Extract windowId from URL parameters if available
			const urlParams = new URLSearchParams(window.location.search);
			const windowId =
				urlParams.get('windowId') ||
				`stream-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

			info(`StreamStore: Using windowId: ${windowId}`);

			// Add a small delay to ensure BroadcastChannel is fully initialized
			setTimeout(() => {
				info(`StreamStore: Sending ready signal for windowId: ${windowId}`);
				broadcastService.sendStreamReady(windowId);
			}, 100);

			info('StreamStore: Initialized and ready to receive broadcast messages');
		}
	}

	function handleStateUpdate(message: BroadcastMessage): void {
		const stateMessage = message as StateUpdateMessage;
		console.log(
			`StreamStore: Received state update for ${stateMessage.gameType}`,
			stateMessage.state
		);
		debug(`StreamStore: Received state update for ${stateMessage.gameType}`);

		// Only update if this is for the current game or we should switch games
		if (state.currentGameType === null || state.currentGameType === stateMessage.gameType) {
			info(`StreamStore: Updating game state for ${stateMessage.gameType}`);
			state.currentGameType = stateMessage.gameType;
			state.gameState = stateMessage.state;
			state.lastUpdateTimestamp = stateMessage.timestamp;
			console.log(
				'StreamStore: Updated game state, current phase:',
				stateMessage.state?.phase?.type
			);
		} else {
			info(
				`StreamStore: Ignoring state update for ${stateMessage.gameType}, current game is ${state.currentGameType}`
			);
		}
	}

	function handleStreamEvent(message: BroadcastMessage): void {
		const eventMessage = message as StreamEventMessage;
		debug(`StreamStore: Received stream event for ${eventMessage.gameType}:`, eventMessage.event);

		// Only process events for the current game
		if (state.currentGameType === eventMessage.gameType) {
			addStreamEvent(eventMessage.event);
		}
	}

	function handleGameChanged(message: BroadcastMessage): void {
		const gameMessage = message as GameChangedMessage;
		info(`StreamStore: Game changed to ${gameMessage.gameType}`);

		// Clear current state when game changes
		state.currentGameType = gameMessage.gameType;
		state.gameState = gameMessage.gameType ? {} : null;
		clearAllStreamEvents();
		state.lastUpdateTimestamp = gameMessage.timestamp;
	}

	function handleStreamControl(message: BroadcastMessage): void {
		const controlMessage = message as StreamControlMessage;
		debug(`StreamStore: Received stream control command: ${controlMessage.command}`);

		switch (controlMessage.command) {
			case 'show':
				state.isVisible = true;
				break;
			case 'hide':
				state.isVisible = false;
				break;
			case 'clear':
				clearAllStreamEvents();
				state.gameState = state.currentGameType ? {} : null;
				break;
		}
	}

	function addStreamEvent(event: StreamEvent): void {
		// Add timestamp if not present
		const eventWithTimestamp = {
			...event,
			timestamp: event.timestamp || Date.now()
		};

		// Remove oldest events if we're at max capacity
		while (state.activeEvents.length >= displayConfig.maxActiveEvents) {
			const oldestEvent = state.activeEvents.shift();
			if (oldestEvent && oldestEvent.timestamp) {
				// Clear any pending timeout for the removed event
				const timeoutId = eventCleanupTimeouts.get(oldestEvent.timestamp.toString());
				if (timeoutId) {
					clearTimeout(timeoutId);
					eventCleanupTimeouts.delete(oldestEvent.timestamp.toString());
				}
			}
		}

		state.activeEvents.push(eventWithTimestamp);

		// Set up automatic removal if duration is specified
		const duration = event.duration || displayConfig.animationDuration;
		if (duration > 0 && eventWithTimestamp.timestamp) {
			const timeoutId = setTimeout(() => {
				removeStreamEvent(eventWithTimestamp.timestamp!);
			}, duration);

			eventCleanupTimeouts.set(eventWithTimestamp.timestamp.toString(), timeoutId);
		}
	}

	function removeStreamEvent(timestamp: number): void {
		const index = state.activeEvents.findIndex((event) => event.timestamp === timestamp);
		if (index !== -1) {
			state.activeEvents.splice(index, 1);

			// Clean up timeout reference
			const timeoutId = eventCleanupTimeouts.get(timestamp.toString());
			if (timeoutId) {
				clearTimeout(timeoutId);
				eventCleanupTimeouts.delete(timestamp.toString());
			}
		}
	}

	function clearAllStreamEvents(): void {
		// Clear all timeouts
		eventCleanupTimeouts.forEach((timeoutId) => clearTimeout(timeoutId));
		eventCleanupTimeouts.clear();

		// Clear events array
		state.activeEvents.length = 0;
	}

	function updateDisplayConfig(newConfig: Partial<StreamDisplayConfig>): void {
		Object.assign(displayConfig, newConfig);
		debug('StreamStore: Display config updated:', displayConfig);
	}

	function cleanup(): void {
		clearAllStreamEvents();
		broadcastService.cleanup();
		info('StreamStore: Cleaned up');
	}

	// Derived states for easier consumption in components
	const hasActiveGame = $derived(state.currentGameType !== null);
	const hasActiveEvents = $derived(state.activeEvents.length > 0);
	const isReady = $derived(hasActiveGame && state.gameState !== null);

	return {
		// State access
		get state() {
			return state;
		},
		get displayConfig() {
			return displayConfig;
		},

		// Derived states
		get hasActiveGame() {
			return hasActiveGame;
		},
		get hasActiveEvents() {
			return hasActiveEvents;
		},
		get isReady() {
			return isReady;
		},

		// Actions
		initialize,
		updateDisplayConfig,
		removeStreamEvent,
		clearAllStreamEvents,
		cleanup
	};
}

export const streamStore = createStreamStore();
