// src/lib/stores/websocket.store.svelte.ts

import { PUBLIC_WS_BASE_URL } from '$env/static/public';
import type {
	ClientToServerMessage,
	ServerToClientMessage,
	GameSpecificEventPayload,
	SystemErrorPayload,
	ConnectToLobbyPayload, // For sending Client -> Server
	ConnectionAckPayload, // For receiving Server -> Client
	GlobalEventPayload,
	TwitchMessageRelayPayload
} from '$lib/types/websocket.types'; // Ensure these types use PascalCase for messageType variants
import { info, warn, error as logError, debug } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';
import { lobbyStore } from './lobby.store.svelte';
import { notificationStore } from './notification.store.svelte';

export enum ConnectionStatus {
	INITIAL = 'INITIAL', // Before any connection attempt
	CONNECTING = 'CONNECTING', // Underlying WebSocket is attempting to open
	AWAITING_CONNECT_ACK = 'AWAITING_CONNECT_ACK', // Socket open, sent ConnectToLobby, waiting for server ack/response
	CONNECTED = 'CONNECTED', // Server has acknowledged/responded positively to ConnectToLobby
	DISCONNECTED = 'DISCONNECTED', // Deliberately disconnected by client or cleanly by server (expected)
	RECONNECTING = 'RECONNECTING', // Attempting to reconnect after an unexpected closure
	ERROR = 'ERROR' // An error occurred preventing connection or during active session
}

interface WebSocketStoreState {
	status: ConnectionStatus;
	lastError: string | null;
	socket: WebSocket | null;
	reconnectAttempts: number;
	currentLobbyId: string | null; // The lobby ID we are trying to connect/are connected to
}

const MAX_RECONNECT_ATTEMPTS = 5;
const INITIAL_RECONNECT_DELAY_MS = 1000;
const MAX_RECONNECT_DELAY_MS = 30000;

function createWebSocketStore() {
	const state = $state<WebSocketStoreState>({
		status: ConnectionStatus.INITIAL,
		lastError: null,
		socket: null,
		reconnectAttempts: 0,
		currentLobbyId: null
	});

	let reconnectTimeoutId: number | undefined;

	// For the Promise returned by the public connect() method
	let currentExternalConnectPromise: {
		resolve: () => void;
		reject: (reason?: any) => void;
	} | null = null;

	function setStatus(newStatus: ConnectionStatus) {
		const previousStatus = state.status;
		if (previousStatus !== newStatus) {
			state.status = newStatus;
			// Notify lobbyStore about the status change for potential UI/state adjustments
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

	function clearAllTimers() {
		if (reconnectTimeoutId) clearTimeout(reconnectTimeoutId);
		reconnectTimeoutId = undefined;
	}

	function _handleExternalPromiseResolution() {
		if (currentExternalConnectPromise) {
			currentExternalConnectPromise.resolve();
			currentExternalConnectPromise = null;
		}
	}

	function _handleExternalPromiseRejection(reasonMessage: string, error?: any) {
		if (currentExternalConnectPromise) {
			currentExternalConnectPromise.reject(error || new Error(reasonMessage));
			currentExternalConnectPromise = null;
		}
	}

	// Internal connect function, not directly returning a promise here,
	// but interacts with currentExternalConnectPromise
	function _initiateConnectionSequence(lobbyIdToConnect: string) {
		// If a socket exists from a previous attempt, ensure it's fully cleaned up.
		if (state.socket) {
			info('Found existing socket, ensuring it is closed before creating new one.');
			// Nullify handlers to prevent them from firing for the old socket
			state.socket.onopen = null;
			state.socket.onmessage = null;
			state.socket.onerror = null;
			state.socket.onclose = null;
			state.socket.close(1000, 'Client re-initiating connection');
			state.socket = null;
		}

		state.currentLobbyId = lobbyIdToConnect; // Track the lobby we're trying for
		const wsUrl = `${PUBLIC_WS_BASE_URL}/ws`;
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

				const connectPayload: ConnectToLobbyPayload = { lobby_id: lobbyIdToConnect };
				// Use PascalCase for messageType if server expects it
				sendRawJsonMessage({ messageType: 'ConnectToLobby', payload: connectPayload });
			};

			newSocket.onmessage = (event: MessageEvent) => {
				try {
					if (typeof event.data !== 'string') {
						warn('WebSocket: Received non-string message, ignoring.', event.data);
						return;
					}

					const message = JSON.parse(event.data) as ServerToClientMessage;
					debug('WebSocket message received:', message);

					// If we were waiting for Connect ACK and get any valid app message (not a SystemError denying connect)
					if (state.status === ConnectionStatus.AWAITING_CONNECT_ACK) {
						if (message.messageType === 'SystemError') {
							const errorMsg = (message.payload as SystemErrorPayload).message;
							logError(
								'WebSocket Store: SystemError received while AWAITING_CONNECT_ACK:',
								errorMsg
							);
							state.lastError = errorMsg;
							setStatus(ConnectionStatus.ERROR);
							_handleExternalPromiseRejection(errorMsg);
							state.socket?.close(4002, 'ConnectToLobby rejected by server'); // Inform server
							return; // Stop processing this message further
						} else {
							// Any other message implies successful logical connection
							info(
								'WebSocket application-level connection confirmed by first valid server message.'
							);
							setStatus(ConnectionStatus.CONNECTED);
							resetReconnectAttempts();
							_handleExternalPromiseResolution();
						}
					}

					// Ensure messageType is PascalCase if that's what server sends
					switch (message.messageType) {
						case 'ConnectionAck':
							if (state.status !== ConnectionStatus.CONNECTED) {
								info('ConnectionAck received. Finalizing connection.');
								setStatus(ConnectionStatus.CONNECTED);
								resetReconnectAttempts();
								_handleExternalPromiseResolution(); // Resolve if not already
							}
							lobbyStore.handleConnectionAck(message.payload as ConnectionAckPayload);
							break;
						case 'GameSpecificEvent':
							gameEventRouter.routeGameSpecificEvent(message.payload as GameSpecificEventPayload);
							break;
						case 'GlobalEvent':
							lobbyStore.handleGlobalEvent(message.payload as GlobalEventPayload);
							break;
						case 'SystemError': // Error during an active session
							lobbyStore.handleSystemError(message.payload as SystemErrorPayload);
							break;
						case 'TwitchMessageRelay':
							lobbyStore.handleTwitchMessageRelay(message.payload as TwitchMessageRelayPayload);
							break;
						// case 'Pong': // Usually handled by browser, but if server sends custom Pong message
						//  debug('Received custom Pong from server.');
						//  break;
						default:
							warn(`WebSocket Store: Received unhandled message type: '${message.messageType}'`);
							notificationStore.add(
								`Received unknown server message type: ${message.messageType}`,
								'warning'
							);
					}
				} catch (e) {
					warn('Failed to parse WebSocket message or handle it:', event.data, e);
					const parseErrorMsg = 'Received malformed message from server.';
					state.lastError = parseErrorMsg;
					notificationStore.add('Received unreadable message from server.', 'destructive');
					if (state.status === ConnectionStatus.AWAITING_CONNECT_ACK) {
						setStatus(ConnectionStatus.ERROR);
						_handleExternalPromiseRejection(parseErrorMsg, e);
					}
				}
			};

			newSocket.onerror = (event: Event) => {
				// This event is often vague. The onclose event provides more details.
				logError('WebSocket native error event occurred. Details should follow in onclose.', event);
				if (!state.lastError) {
					// Preserve more specific error if already set
					state.lastError = 'A WebSocket connection error occurred.';
				}
				// Do not change status or reject promise here; onclose will handle it.
			};

			newSocket.onclose = (event: CloseEvent) => {
				info(
					`WebSocket connection closed. Code: ${event.code}, Reason: '${event.reason || 'No reason given'}', Clean: ${event.wasClean}`
				);
				const statusBeforeClose = state.status;
				state.socket = null; // Clear the socket reference

				// If disconnect was manually initiated by client, currentExternalConnectPromise should already be null or rejected.
				if (statusBeforeClose === ConnectionStatus.DISCONNECTED) {
					info('WebSocket closure was due to manual disconnect. No further action.');
					_handleExternalPromiseRejection('Manually disconnected'); // Ensure any lingering promise is rejected
					resetReconnectAttempts(); // No reconnects on manual disconnect
					return;
				}

				// Determine the error message
				const closeReasonMessage =
					state.lastError || // Use pre-existing error if available
					`Connection closed (Code: ${event.code}${event.reason ? ` - Reason: ${event.reason}` : ''})`;
				state.lastError = closeReasonMessage;

				// If the promise from connect() is still pending, reject it.
				_handleExternalPromiseRejection(
					closeReasonMessage,
					new Error(`WebSocket closed with code ${event.code}: ${event.reason}`)
				);

				// Decide whether to attempt reconnection
				// Don't reconnect for certain server-initiated close codes if they imply non-transient issues (e.g., 4xxx codes)
				// Code 1000: Normal Closure
				// Code 1006: Abnormal Closure (often network issue or server crash)
				const shouldAttemptReconnect = event.code !== 1000 && event.code < 4000;

				if (shouldAttemptReconnect) {
					attemptReconnectAfterDelay();
				} else {
					setStatus(ConnectionStatus.ERROR); // Or DISCONNECTED if it was a clean, expected server close (1000)
					resetReconnectAttempts();
					if (event.code === 1000 && statusBeforeClose === ConnectionStatus.CONNECTED) {
						notificationStore.add('Connection cleanly closed by server.', 'info');
					}
				}
			};
		} catch (err) {
			logError('Failed to create WebSocket instance:', err);
			const initErrorMsg = 'Failed to initialize WebSocket connection.';
			setStatus(ConnectionStatus.ERROR);
			state.lastError = initErrorMsg;
			_handleExternalPromiseRejection(initErrorMsg, err);
			state.socket = null;
			// Optionally attempt reconnect even here if currentLobbyId is set, though it implies a fundamental client issue.
			// if (state.currentLobbyId) attemptReconnectAfterDelay();
		}
	}

	function attemptReconnectAfterDelay() {
		if (!state.currentLobbyId) {
			warn(
				'Cannot reconnect: currentLobbyId not available (likely after manual disconnect or severe error).'
			);
			setStatus(ConnectionStatus.ERROR); // Ensure final error state
			state.lastError = state.lastError || 'Reconnection details lost.';
			resetReconnectAttempts();
			// No promise to reject here as this is part of internal retry, not initial connect()
			return;
		}

		if (state.reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
			warn(
				`Max reconnect attempts (${MAX_RECONNECT_ATTEMPTS}) reached for lobby ${state.currentLobbyId}. Stopping.`
			);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = state.lastError || 'Failed to reconnect after multiple attempts.';
			resetReconnectAttempts();
			// No promise to reject here for the same reason
			return;
		}

		state.reconnectAttempts++;
		const delay = Math.min(
			INITIAL_RECONNECT_DELAY_MS * Math.pow(2, state.reconnectAttempts - 1),
			MAX_RECONNECT_DELAY_MS
		);

		info(
			`Attempting reconnect ${state.reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS} for lobby ${state.currentLobbyId} in ${delay / 1000}s...`
		);
		setStatus(ConnectionStatus.RECONNECTING); // This status is important for UI feedback

		reconnectTimeoutId = window.setTimeout(() => {
			if (state.status === ConnectionStatus.DISCONNECTED) {
				// Check if manually disconnected during timeout
				info('Reconnect attempt cancelled due to manual disconnect.');
				return;
			}
			if (state.currentLobbyId) {
				// _initiateConnectionSequence will set up its own promise handling for this specific attempt
				// The original external promise from connect() would have already been rejected.
				_initiateConnectionSequence(state.currentLobbyId);
			} else {
				warn('Reconnect aborted during timeout: currentLobbyId became null.');
				setStatus(ConnectionStatus.ERROR);
				state.lastError = 'Lost reconnection details during backoff.';
			}
		}, delay);
	}

	// --- Public API ---

	async function connect(lobbyId: string): Promise<void> {
		if (!lobbyId) {
			const msg = 'WebSocket connect called without a lobbyId.';
			logError(msg);
			notificationStore.add('Cannot connect: Lobby ID is missing.', 'destructive');
			return Promise.reject(new Error(msg));
		}
		info(`Application requested WebSocket connection for lobby: ${lobbyId}`);

		// If a previous connection attempt is in progress (has an unresolved promise)
		// or if a socket is active, reject/disconnect it first.
		if (currentExternalConnectPromise) {
			info('New connect call while previous connection promise is pending. Rejecting previous.');
			_handleExternalPromiseRejection('New connection attempt initiated.');
		}
		if (state.socket) {
			info('New connect call while socket exists. Disconnecting previous.');
			// Manual disconnect to prevent its onclose from interfering with the new attempt's logic
			disconnectSocketInternally(1000, 'Superseded by new connect call');
		}

		clearAllTimers();
		resetReconnectAttempts();
		setStatus(ConnectionStatus.INITIAL); // Reset status for a fresh attempt

		// Return a new promise that will be resolved/rejected by _initiateConnectionSequence's handlers
		return new Promise<void>((resolve, reject) => {
			currentExternalConnectPromise = { resolve, reject };
			_initiateConnectionSequence(lobbyId);
		});
	}

	// Internal helper to close socket without triggering reconnects from its onclose
	function disconnectSocketInternally(code?: number, reason?: string) {
		if (state.socket) {
			info(`Internally closing socket. Code: ${code}, Reason: ${reason}`);
			state.socket.onclose = null; // Prevent onclose handler
			state.socket.onerror = null; // Prevent onerror handler
			state.socket.onmessage = null;
			state.socket.onopen = null;
			state.socket.close(code, reason);
			state.socket = null;
		}
	}

	function disconnect(): void {
		info('Application requested WebSocket manual disconnect.');
		_handleExternalPromiseRejection('Manually disconnected by client action.'); // Reject any pending connect() promise
		clearAllTimers();
		resetReconnectAttempts(); // No reconnects after manual disconnect

		disconnectSocketInternally(1000, 'Client initiated disconnect');

		setStatus(ConnectionStatus.DISCONNECTED); // Set final status
		state.currentLobbyId = null; // Clear lobby context
		state.lastError = null;
	}

	// Helper for sending the initial ConnectToLobby message, ensuring socket is open.
	function sendRawJsonMessage(message: ClientToServerMessage): void {
		if (state.socket && state.socket.readyState === WebSocket.OPEN) {
			try {
				const messageString = JSON.stringify(message);
				state.socket.send(messageString);
				debug('Raw WebSocket message sent:', message);
			} catch (e) {
				logError('Failed to stringify or send raw WebSocket message:', message, e);
				const sendErrorMsg = 'Failed to send initial connection message.';
				state.lastError = sendErrorMsg;
				notificationStore.add('Error sending initial message to server.', 'destructive');
				setStatus(ConnectionStatus.ERROR);
				_handleExternalPromiseRejection(sendErrorMsg, e); // Reject connect promise
				state.socket?.close(4001, 'Failed to send ConnectToLobby message');
			}
		} else {
			const notOpenMsg =
				'WebSocket not open when trying to send raw message (e.g. ConnectToLobby).';
			warn(notOpenMsg, 'Status:', state.status, 'ReadyState:', state.socket?.readyState);
			state.lastError = notOpenMsg;
			setStatus(ConnectionStatus.ERROR);
			_handleExternalPromiseRejection(notOpenMsg); // Reject connect promise
		}
	}

	// Public method for sending game messages after connection is established
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
				// Optionally, handle this more gracefully, maybe set status to ERROR if send fails repeatedly
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
