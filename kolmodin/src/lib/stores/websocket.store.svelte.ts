// src/lib/stores/websocket.store.svelte.ts

import { PUBLIC_WS_BASE_URL } from '$env/static/public';
// Ensure this import path and types are correct after changes
import type {
	ClientToServerMessage,
	ServerToClientMessage,
	GameSpecificEventPayload,
	SystemErrorPayload,
	ConnectToLobbyPayload, // New payload type
	ConnectionAckPayload, // For server's ack
	GlobalEventPayload,
	TwitchMessageRelayPayload
} from '$lib/types/websocket.types';
import { info, warn, error as logError, debug } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';
import { lobbyStore } from './lobby.store.svelte';
import { notificationStore } from './notification.store.svelte';

export enum ConnectionStatus {
	INITIAL = 'INITIAL',
	CONNECTING = 'CONNECTING', // Underlying WebSocket is attempting to open
	AWAITING_CONNECT_ACK = 'AWAITING_CONNECT_ACK', // Socket open, sent ConnectToLobby, waiting for server ack/response
	CONNECTED = 'CONNECTED', // Server has acknowledged/responded positively to ConnectToLobby
	DISCONNECTED = 'DISCONNECTED',
	RECONNECTING = 'RECONNECTING',
	ERROR = 'ERROR'
}

interface WebSocketStoreState {
	status: ConnectionStatus;
	lastError: string | null;
	socket: WebSocket | null;
	reconnectAttempts: number;
	// currentLobbyId is important for sending the ConnectToLobby message
	// and for knowing which lobby we *think* we are connected to.
	currentLobbyId: string | null;
}

const MAX_RECONNECT_ATTEMPTS = 5;
const INITIAL_RECONNECT_DELAY = 1000;
const MAX_RECONNECT_DELAY = 30000;
// Server pings, client automatically Pongs. Binary heartbeat is optional for client.
// const HEARTBEAT_INTERVAL = 25000;
// const HEARTBEAT_BYTE_CLIENT_SEND = 0x42;

function createWebSocketStore() {
	const state = $state<WebSocketStoreState>({
		status: ConnectionStatus.INITIAL,
		lastError: null,
		socket: null,
		reconnectAttempts: 0,
		currentLobbyId: null // Store the lobby ID we intend to connect to
	});

	let reconnectTimeoutId: number | undefined;
	// let clientHeartbeatIntervalId: number | undefined; // Optional binary heartbeat

	function setStatus(newStatus: ConnectionStatus) {
		const previousStatus = state.status;
		if (previousStatus !== newStatus) {
			state.status = newStatus;
			// Notify lobbyStore about the status change if it needs to react
			lobbyStore.handleWebSocketStatusChange(newStatus, previousStatus);
			debug(`WebSocket status changed from ${previousStatus} -> ${newStatus}`);
		}
	}

	function resetReconnectAttempts() {
		state.reconnectAttempts = 0;
		if (reconnectTimeoutId) {
			clearTimeout(reconnectTimeoutId);
			reconnectTimeoutId = undefined;
		}
	}

	function clearTimers() {
		if (reconnectTimeoutId) clearTimeout(reconnectTimeoutId);
		// if (clientHeartbeatIntervalId) clearInterval(clientHeartbeatIntervalId);
		reconnectTimeoutId = undefined;
		// clientHeartbeatIntervalId = undefined;
	}

	// Client-initiated binary heartbeat is optional. Server already Pings.
	// function startClientHeartbeat() { /* ... */ }
	// function stopClientHeartbeat() { /* ... */ }

	// The `lobbyIdToConnect` is the one received from the `/api/create-lobby` endpoint.
	function _connect(lobbyIdToConnect: string) {
		if (
			state.status === ConnectionStatus.CONNECTING ||
			state.status === ConnectionStatus.RECONNECTING ||
			state.status === ConnectionStatus.AWAITING_CONNECT_ACK ||
			(state.status === ConnectionStatus.CONNECTED && state.currentLobbyId === lobbyIdToConnect)
		) {
			info(
				`WebSocket connection attempt for lobby ${lobbyIdToConnect} already in progress or completed with this ID.`
			);
			return;
		}

		clearTimers();
		state.currentLobbyId = lobbyIdToConnect; // Store the lobby ID we will send in ConnectToLobby

		const wsUrl = `${PUBLIC_WS_BASE_URL}/ws`; // Generic WebSocket endpoint
		info(`Attempting to connect to WebSocket: ${wsUrl} for lobby ${lobbyIdToConnect}`);
		setStatus(
			state.reconnectAttempts > 0 ? ConnectionStatus.RECONNECTING : ConnectionStatus.CONNECTING
		);
		state.lastError = null;

		try {
			const newSocket = new WebSocket(wsUrl);
			state.socket = newSocket;

			newSocket.onopen = () => {
				info(
					`WebSocket underlying connection open for lobby ${lobbyIdToConnect}. Sending ConnectToLobby message...`
				);
				setStatus(ConnectionStatus.AWAITING_CONNECT_ACK);
				state.lastError = null;

				// Send the ConnectToLobby message
				const connectPayload: ConnectToLobbyPayload = { lobby_id: lobbyIdToConnect };
				sendRawJson({ messageType: 'ConnectToLobby', payload: connectPayload });
			};

			newSocket.onmessage = (event: MessageEvent) => {
				try {
					if (typeof event.data !== 'string') {
						warn(
							'WebSocket: Received non-string message, ignoring (could be binary heartbeat).',
							event.data
						);
						return;
					}

					const message = JSON.parse(event.data) as ServerToClientMessage;
					debug('WebSocket message received:', message);

					if (state.status === ConnectionStatus.AWAITING_CONNECT_ACK) {
						// If we receive a SystemError here, it might be due to the ConnectToLobby failing.
						if (message.messageType === 'SystemError') {
							logError(
								'WebSocket Store: SystemError received while AWAITING_CONNECT_ACK:',
								(message.payload as SystemErrorPayload).message
							);
							state.lastError = (message.payload as SystemErrorPayload).message;
							setStatus(ConnectionStatus.ERROR); // Connection failed
							state.socket?.close(4002, 'ConnectToLobby processing failed by server');
							// attemptReconnect will be called by onclose if appropriate
							return; // Stop processing this message further if it's an error during handshake
						} else {
							// Any other valid message means the server accepted our ConnectToLobby
							info(
								'WebSocket application-level connection confirmed (received first valid message post-ConnectToLobby).'
							);
							setStatus(ConnectionStatus.CONNECTED);
							resetReconnectAttempts();
							// startClientHeartbeat(); // Optional
						}
					}

					switch (message.messageType) {
						case 'ConnectionAck': // Explicit ack from server
							if (state.status !== ConnectionStatus.CONNECTED) {
								info('ConnectionAck received. Finalizing connection.');
								setStatus(ConnectionStatus.CONNECTED);
								resetReconnectAttempts();
								// startClientHeartbeat(); // Optional
							}
							lobbyStore.handleConnectionAck(message.payload as ConnectionAckPayload);
							break;
						case 'GameSpecificEvent':
							gameEventRouter.routeGameSpecificEvent(message.payload as GameSpecificEventPayload);
							break;
						case 'GlobalEvent':
							lobbyStore.handleGlobalEvent(message.payload as GlobalEventPayload);
							break;
						case 'SystemError':
							// Already handled if it occurred during AWAITING_CONNECT_ACK
							// If it occurs later, it's a runtime error.
							lobbyStore.handleSystemError(message.payload as SystemErrorPayload);
							break;
						case 'TwitchMessageRelay':
							lobbyStore.handleTwitchMessageRelay(message.payload as TwitchMessageRelayPayload);
							break;
						case 'Pong': // If server sends explicit Pong message type
							debug('Received Pong from server.');
							break;
						default:
							warn(
								`WebSocket Store: Received unhandled message type: ${(message as any).messageType}`
							);
							notificationStore.add(
								`Received unknown server message: ${(message as any).messageType}`,
								'warning'
							);
					}
				} catch (e) {
					warn('Failed to parse WebSocket message or handle it:', event.data, e);
					state.lastError = 'Received malformed message from server.';
					notificationStore.add('Received unreadable message from server.', 'destructive');
				}
			};

			newSocket.onerror = (event: Event) => {
				logError('WebSocket error event occurred. See onclose for status update.', event);
				// Don't set lastError here if it was already set by a SystemError, preserve original error
				if (!state.lastError) {
					state.lastError = 'WebSocket connection error.';
				}
				// `onclose` will usually follow.
			};

			newSocket.onclose = (event: CloseEvent) => {
				info(
					`WebSocket connection closed. Code: ${event.code}, Reason: '${event.reason}', Clean: ${event.wasClean}`
				);
				const statusBeforeClose = state.status;
				// stopClientHeartbeat(); // Optional
				state.socket = null;

				if (statusBeforeClose === ConnectionStatus.DISCONNECTED) {
					// Manually disconnected by client
					info('WebSocket was manually disconnected.');
					// setStatus(ConnectionStatus.DISCONNECTED) was already called.
					resetReconnectAttempts(); // Don't reconnect if manual
					return;
				}

				if (!state.lastError) {
					// If no specific error message was set before closing
					state.lastError = `Connection closed (Code: ${event.code}${event.reason ? ` - Reason: ${event.reason}` : ''})`;
				}

				// If not a clean close (1000) or if we were in a connecting/awaiting phase, try to reconnect.
				// Also, if the server explicitly closed with a "lobby not found" or similar, we might not want to retry.
				// For now, we retry on any non-1000 code or if closed during handshake.
				if (
					event.code !== 1000 ||
					statusBeforeClose === ConnectionStatus.CONNECTING ||
					statusBeforeClose === ConnectionStatus.AWAITING_CONNECT_ACK
				) {
					attemptReconnect();
				} else {
					// Clean closure (code 1000) and we were previously CONNECTED
					if (statusBeforeClose === ConnectionStatus.CONNECTED) {
						notificationStore.add('Connection cleanly closed by server.', 'info');
					}
					setStatus(ConnectionStatus.ERROR); // Or DISCONNECTED if it's a graceful server-side lobby end
					state.lastError = state.lastError || 'Server closed the connection.';
					resetReconnectAttempts();
				}
			};
		} catch (err) {
			logError('Failed to create WebSocket instance:', err);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Failed to initialize WebSocket connection.';
			state.socket = null;
			if (state.currentLobbyId) attemptReconnect();
		}
	}

	function attemptReconnect() {
		if (!state.currentLobbyId) {
			warn('Cannot reconnect: currentLobbyId for ConnectToLobby message not available.');
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Reconnection details (currentLobbyId) lost.';
			resetReconnectAttempts();
			return;
		}

		if (state.reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
			warn(`Max reconnect attempts (${MAX_RECONNECT_ATTEMPTS}) reached. Stopping.`);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = state.lastError || 'Failed to reconnect after multiple attempts.';
			resetReconnectAttempts();
			return;
		}

		state.reconnectAttempts++;
		const delay = Math.min(
			INITIAL_RECONNECT_DELAY * Math.pow(2, state.reconnectAttempts - 1),
			MAX_RECONNECT_DELAY
		);

		info(
			`Attempting reconnect ${state.reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS} for lobby ${state.currentLobbyId} in ${delay / 1000}s...`
		);
		setStatus(ConnectionStatus.RECONNECTING);

		reconnectTimeoutId = window.setTimeout(() => {
			if (state.status === ConnectionStatus.DISCONNECTED) {
				// Check if manually disconnected during timeout
				info('Reconnect attempt cancelled due to manual disconnect.');
				return;
			}
			if (state.currentLobbyId) {
				_connect(state.currentLobbyId);
			} else {
				warn('Reconnect aborted during timeout: currentLobbyId became null.');
				setStatus(ConnectionStatus.ERROR);
				state.lastError = 'Lost reconnection details during backoff (currentLobbyId).';
			}
		}, delay);
	}

	// Public connect method, takes the lobbyId from createLobby response
	function connect(lobbyId: string): void {
		if (!lobbyId) {
			logError('WebSocket connect called without a lobbyId.');
			notificationStore.add('Cannot connect: Lobby ID is missing.', 'destructive');
			return;
		}
		info(`Application requested WebSocket connection for lobby: ${lobbyId}`);
		clearTimers();
		resetReconnectAttempts();

		if (state.socket) {
			// If there's an old socket, ensure it's fully closed
			info('An existing WebSocket is present. Closing it before new connect.');
			// Remove old listeners to prevent them firing on the old socket after a new connect call
			state.socket.onopen = null;
			state.socket.onmessage = null;
			state.socket.onerror = null;
			state.socket.onclose = null; // Critical to prevent old onclose from triggering reconnect
			state.socket.close(1000, 'Client initiating new connection');
			state.socket = null;
		}
		setStatus(ConnectionStatus.INITIAL); // Reset status before new attempt
		_connect(lobbyId);
	}

	function disconnect(): void {
		info('Application requested WebSocket disconnect.');
		const oldStatus = state.status;
		clearTimers();
		resetReconnectAttempts();

		if (state.socket) {
			setStatus(ConnectionStatus.DISCONNECTED); // Set status *before* closing
			state.socket.close(1000, 'Client initiated disconnect');
			// onclose handler will set state.socket to null
		} else {
			// If no socket, but status wasn't DISCONNECTED, set it now.
			if (oldStatus !== ConnectionStatus.DISCONNECTED) {
				setStatus(ConnectionStatus.DISCONNECTED);
			}
		}
		state.currentLobbyId = null; // Clear current lobby ID on manual disconnect
	}

	// Helper to send raw JSON, used internally by _connect for ConnectToLobby
	function sendRawJson(message: ClientToServerMessage): void {
		if (state.socket && state.socket.readyState === WebSocket.OPEN) {
			// Check open, not full CONNECTED status
			try {
				const messageString = JSON.stringify(message);
				state.socket.send(messageString);
				debug('Raw WebSocket message sent (e.g. ConnectToLobby):', message);
			} catch (e) {
				logError('Failed to stringify or send raw WebSocket message:', message, e);
				// This is critical if it fails during ConnectToLobby
				state.lastError = 'Failed to send initial connection message.';
				notificationStore.add('Error sending initial message to server.', 'destructive');
				setStatus(ConnectionStatus.ERROR);
				state.socket?.close(4001, 'Failed to send ConnectToLobby');
			}
		} else {
			warn('WebSocket not open. Cannot send raw message:', message);
			// This path should ideally not be hit for ConnectToLobby due to onopen guard
		}
	}

	// Public send method for general game messages, requires full CONNECTED status
	function send(message: ClientToServerMessage): void {
		if (
			state.socket &&
			state.socket.readyState === WebSocket.OPEN &&
			state.status === ConnectionStatus.CONNECTED
		) {
			try {
				const messageString = JSON.stringify(message);
				state.socket.send(messageString);
				debug('WebSocket message sent:', message);
			} catch (e) {
				logError('Failed to stringify or send WebSocket message:', message, e);
				state.lastError = 'Failed to send message.';
				notificationStore.add('Error sending message to server.', 'destructive');
			}
		} else {
			warn(
				'WebSocket not open or not fully connected. Cannot send message:',
				message,
				'Status:',
				state.status
			);
			state.lastError = 'Connection not available to send message.';
			notificationStore.add('Cannot send message: Not fully connected to server.', 'warning');
		}
	}

	return {
		get state() {
			return state;
		},
		connect,
		disconnect,
		send
	};
}

export const websocketStore = createWebSocketStore();
