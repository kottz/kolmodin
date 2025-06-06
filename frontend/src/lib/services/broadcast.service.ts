// src/lib/services/broadcast.service.ts

import { debug, info, warn } from '$lib/utils/logger';
import type { BroadcastMessage, StreamEvent } from '$lib/types/stream.types';

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

	function initialize(isStreamWindow: boolean = false): void {
		if (state.channel) {
			warn('BroadcastService: Channel already initialized, closing existing one.');
			cleanup();
		}

		try {
			state.channel = new BroadcastChannel(BROADCAST_CHANNEL_NAME);
			state.isStreamWindow = isStreamWindow;

			state.channel.onmessage = (event) => {
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

		// Don't process messages if we're the admin window (sender)
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

		// Only admin window should send messages
		if (state.isStreamWindow) {
			warn('BroadcastService: Stream window attempted to send message, ignoring');
			return;
		}

		try {
			state.channel.postMessage(message);
			debug('BroadcastService: Sent message:', message);
		} catch (error) {
			warn('BroadcastService: Failed to send message:', error);
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

	function broadcastStateUpdate(gameType: string, publicState: any): void {
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

	function cleanup(): void {
		if (state.channel) {
			state.channel.close();
			state.channel = null;
		}
		state.messageHandlers.clear();
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
		isInitialized,
		getIsStreamWindow
	};
}

export const broadcastService = createBroadcastService();
