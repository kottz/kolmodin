// src/lib/services/broadcast.service.ts

import { debug, info, warn } from '$lib/utils/logger';
import type { BroadcastMessage, StreamEvent, StateUpdateMessage } from '$lib/types/stream.types';

const BROADCAST_CHANNEL_NAME = 'kolmodin-stream';

interface BroadcastServiceState {
	channel: BroadcastChannel | null;
	isStreamWindow: boolean;
	messageHandlers: Map<string, (message: BroadcastMessage) => void>;
}

function createBroadcastService() {
	const state: BroadcastServiceState = {
		channel: null,
		isStreamWindow: false,
		messageHandlers: new Map()
	};

	// Stream confirmation tracking
	const streamConfirmation = {
		pendingWindowId: null as string | null,
		confirmationReceived: false,
		confirmationTimeout: null as number | null
	};

	function initialize(isStreamWindow: boolean = false): void {
		if (state.channel) {
			warn('BroadcastService: Channel already initialized, closing existing one.');
			cleanup();
		}

		try {
			state.channel = new BroadcastChannel(BROADCAST_CHANNEL_NAME);
			state.isStreamWindow = isStreamWindow;

			state.channel.onmessage = (event) => {
				debug(`BroadcastService: Raw message received:`, event.data);
				handleIncomingMessage(event.data as BroadcastMessage);
			};

			state.channel.onerror = (error) => {
				warn('BroadcastService: Channel error:', error);
			};

			info(`BroadcastService: Initialized as ${isStreamWindow ? 'stream' : 'admin'} window`);
		} catch (error) {
			warn('BroadcastService: Failed to initialize BroadcastChannel:', error);
		}
	}

	function handleIncomingMessage(message: BroadcastMessage): void {
		debug('BroadcastService: Received message:', message);

		// Handle confirmation messages regardless of window type
		if (message.type === 'STREAM_READY') {
			handleConfirmationMessage(message);
			return;
		}

		// Don't process other messages if we're the admin window (sender)
		if (!state.isStreamWindow) {
			return;
		}

		// Route message to appropriate handler
		const handler = state.messageHandlers.get(message.type);
		if (handler) {
			try {
				handler(message);
			} catch (error) {
				warn(`BroadcastService: Error handling message type ${message.type}:`, error);
			}
		} else {
			warn(`BroadcastService: No handler registered for message type: ${message.type}`);
		}
	}

	function sendMessage(message: BroadcastMessage): void {
		if (!state.channel) {
			warn('BroadcastService: Cannot send message, channel not initialized');
			return;
		}

		// Stream windows can only send STREAM_READY messages
		if (state.isStreamWindow && message.type !== 'STREAM_READY') {
			warn('BroadcastService: Stream window attempted to send non-STREAM_READY message, ignoring');
			return;
		}

		try {
			// Test if message is serializable first
			JSON.parse(JSON.stringify(message));
			state.channel.postMessage(message);
			debug('BroadcastService: Sent message:', message);
		} catch (error) {
			if (error instanceof TypeError && error.message.includes('could not be cloned')) {
				console.error('BroadcastService: Message contains non-serializable data:', error);
				console.error('Problematic message:', message);
				// Try to send a simplified version for STATE_UPDATE messages
				if (message.type === 'STATE_UPDATE') {
					const stateMessage = message as StateUpdateMessage;
					const simpleMessage = {
						type: 'STATE_UPDATE',
						gameType: stateMessage.gameType,
						state: {
							phase: {
								type:
									(stateMessage.state as { phase?: { type?: string } })?.phase?.type || 'Unknown'
							}
						},
						timestamp: Date.now()
					};
					try {
						state.channel.postMessage(simpleMessage);
						console.log('BroadcastService: Sent simplified message instead');
					} catch (fallbackError) {
						warn('BroadcastService: Even simplified message failed:', fallbackError);
					}
				}
			} else {
				warn('BroadcastService: Failed to send message:', error);
			}
		}
	}

	function registerMessageHandler(
		messageType: string,
		handler: (message: BroadcastMessage) => void
	): void {
		state.messageHandlers.set(messageType, handler);
		debug(`BroadcastService: Registered handler for message type: ${messageType}`);
	}

	function unregisterMessageHandler(messageType: string): void {
		state.messageHandlers.delete(messageType);
		debug(`BroadcastService: Unregistered handler for message type: ${messageType}`);
	}

	function broadcastStateUpdate(gameType: string, publicState: unknown): void {
		console.log(
			'BroadcastService: Broadcasting state update for',
			gameType,
			'with phase:',
			(publicState as { phase?: { type?: string } })?.phase?.type
		);
		sendMessage({
			type: 'STATE_UPDATE',
			gameType,
			state: publicState,
			timestamp: Date.now()
		});
	}

	function broadcastStreamEvent(gameType: string, event: StreamEvent): void {
		sendMessage({
			type: 'STREAM_EVENT',
			gameType,
			event,
			timestamp: Date.now()
		});
	}

	function broadcastGameChanged(gameType: string | null): void {
		sendMessage({
			type: 'GAME_CHANGED',
			gameType,
			timestamp: Date.now()
		});
	}

	function broadcastStreamControl(command: 'show' | 'hide' | 'clear'): void {
		sendMessage({
			type: 'STREAM_CONTROL',
			command,
			timestamp: Date.now()
		});
	}

	// Stream confirmation functions
	function handleConfirmationMessage(message: BroadcastMessage): void {
		debug(`BroadcastService: Handling confirmation message:`, message);

		if (message.type === 'STREAM_READY') {
			// Stream window is signaling it's ready
			const readyMessage = message as import('$lib/types/stream.types').StreamReadyMessage;
			info(`BroadcastService: Stream window ${readyMessage.windowId} is ready`);

			// If we're the admin window waiting for this specific window, mark as confirmed
			if (!state.isStreamWindow && streamConfirmation.pendingWindowId === readyMessage.windowId) {
				streamConfirmation.confirmationReceived = true;
				info(`BroadcastService: Confirmed stream window ${readyMessage.windowId}`);
			}
		}
	}

	function sendStreamReady(windowId: string): void {
		info(`BroadcastService: Stream window sending STREAM_READY for window ${windowId}`);

		sendMessage({
			type: 'STREAM_READY',
			windowId,
			timestamp: Date.now()
		});
	}

	function waitForStreamConfirmation(windowId: string): Promise<boolean> {
		return new Promise((resolve) => {
			info(`BroadcastService: Admin waiting for confirmation from window ${windowId}`);

			// Set up tracking for this window
			streamConfirmation.pendingWindowId = windowId;
			streamConfirmation.confirmationReceived = false;

			// Poll for confirmation
			const checkInterval = setInterval(() => {
				if (
					streamConfirmation.confirmationReceived &&
					streamConfirmation.pendingWindowId === windowId
				) {
					clearInterval(checkInterval);
					if (streamConfirmation.confirmationTimeout) {
						clearTimeout(streamConfirmation.confirmationTimeout);
						streamConfirmation.confirmationTimeout = null;
					}
					resolve(true);
				}
			}, 100);

			// Set timeout for confirmation
			streamConfirmation.confirmationTimeout = setTimeout(() => {
				clearInterval(checkInterval);
				warn(`BroadcastService: No acknowledgment received for window ${windowId} within timeout`);
				streamConfirmation.pendingWindowId = null;
				streamConfirmation.confirmationReceived = false;
				streamConfirmation.confirmationTimeout = null;
				resolve(false);
			}, 5000); // 5 second timeout
		});
	}

	function cleanup(): void {
		if (state.channel) {
			state.channel.close();
			state.channel = null;
		}
		state.messageHandlers.clear();

		// Clear confirmation state
		if (streamConfirmation.confirmationTimeout) {
			clearTimeout(streamConfirmation.confirmationTimeout);
		}
		streamConfirmation.pendingWindowId = null;
		streamConfirmation.confirmationReceived = false;
		streamConfirmation.confirmationTimeout = null;

		info('BroadcastService: Cleaned up');
	}

	function isInitialized(): boolean {
		return state.channel !== null;
	}

	function getIsStreamWindow(): boolean {
		return state.isStreamWindow;
	}

	return {
		initialize,
		cleanup,
		sendMessage,
		registerMessageHandler,
		unregisterMessageHandler,
		broadcastStateUpdate,
		broadcastStreamEvent,
		broadcastGameChanged,
		broadcastStreamControl,
		sendStreamReady,
		waitForStreamConfirmation,
		isInitialized,
		getIsStreamWindow
	};
}

export const broadcastService = createBroadcastService();
