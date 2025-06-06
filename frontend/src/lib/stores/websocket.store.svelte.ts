// src/lib/stores/websocket.store.svelte.ts

import { PUBLIC_WS_BASE_URL } from '$env/static/public';
import type {
	ClientToServerMessage,
	ServerToClientMessage,
	GameSpecificEventPayload,
	SystemErrorPayload,
	ConnectToLobbyPayload,
	ConnectionAckPayload,
	GlobalEventPayload,
	TwitchMessageRelayPayload
} from '$lib/types/websocket.types';
import { info, warn, error as logError, debug } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';
import { lobbyStore } from './lobby.store.svelte';
import { notificationStore } from './notification.store.svelte';

export enum ConnectionStatus {
	INITIAL = 'INITIAL',
	CONNECTING = 'CONNECTING',
	AWAITING_CONNECT_ACK = 'AWAITING_CONNECT_ACK',
	CONNECTED = 'CONNECTED',
	DISCONNECTED = 'DISCONNECTED',
	RECONNECTING = 'RECONNECTING',
	ERROR = 'ERROR'
}

interface WebSocketStoreState {
	status: ConnectionStatus;
	lastError: string | null;
	socket: WebSocket | null;
	reconnectAttempts: number;
	currentLobbyId: string | null;
	wasManuallyDisconnected: boolean; // <<< ADDED: Tracks if the last disconnect was user-initiated
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
		currentLobbyId: null,
		wasManuallyDisconnected: false // <<< ADDED: Initialize to false
	});

	let reconnectTimeoutId: number | undefined;
	let currentExternalConnectPromise: {
		resolve: () => void;
		reject: (reason?: any) => void;
	} | null = null;

	function setStatus(newStatus: ConnectionStatus) {
		const previousStatus = state.status;
		if (previousStatus !== newStatus) {
			state.status = newStatus;
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

	function _initiateConnectionSequence(lobbyIdToConnect: string) {
		if (state.socket) {
			info('Found existing socket, ensuring it is closed before creating new one.');
			state.socket.onopen = null;
			state.socket.onmessage = null;
			state.socket.onerror = null;
			state.socket.onclose = null;
			state.socket.close(1000, 'Client re-initiating connection');
			state.socket = null;
		}

		state.currentLobbyId = lobbyIdToConnect;
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
							state.socket?.close(4002, 'ConnectToLobby rejected by server');
							return;
						} else {
							info(
								'WebSocket application-level connection confirmed by first valid server message.'
							);
							setStatus(ConnectionStatus.CONNECTED);
							resetReconnectAttempts();
							_handleExternalPromiseResolution();
						}
					}

					switch (message.messageType) {
						case 'ConnectionAck':
							if (state.status !== ConnectionStatus.CONNECTED) {
								info('ConnectionAck received. Finalizing connection.');
								setStatus(ConnectionStatus.CONNECTED);
								resetReconnectAttempts();
								_handleExternalPromiseResolution();
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
							lobbyStore.handleSystemError(message.payload as SystemErrorPayload);
							break;
						case 'TwitchMessageRelay':
							lobbyStore.handleTwitchMessageRelay(message.payload as TwitchMessageRelayPayload);
							break;
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
				logError('WebSocket native error event occurred. Details should follow in onclose.', event);
				if (!state.lastError) {
					state.lastError = 'A WebSocket connection error occurred.';
				}
			};

			newSocket.onclose = (event: CloseEvent) => {
				info(
					`WebSocket connection closed. Code: ${event.code}, Reason: '${event.reason || 'No reason given'}', Clean: ${event.wasClean}`
				);
				const statusBeforeClose = state.status;
				state.socket = null;

				// If disconnect() was called, wasManuallyDisconnected will be true.
				// disconnect() also sets onclose=null for the socket it's closing,
				// so this handler ideally shouldn't run for that specific socket instance's closure.
				// However, this check provides robustness.
				if (state.wasManuallyDisconnected && statusBeforeClose === ConnectionStatus.DISCONNECTED) {
					info(
						'WebSocket closure was due to manual disconnect (confirmed by flag). No further action from onclose.'
					);
					return;
				}
				// This also covers if status was CLOSING_INTENTIONALLY and then became DISCONNECTED before onclose flag check.

				const closeReasonMessage =
					state.lastError ||
					`Connection closed (Code: ${event.code}${event.reason ? ` - Reason: ${event.reason}` : ''})`;
				state.lastError = closeReasonMessage;
				_handleExternalPromiseRejection(
					closeReasonMessage,
					new Error(`WebSocket closed with code ${event.code}: ${event.reason}`)
				);

				const shouldAttemptReconnect = event.code !== 1000 && event.code < 4000;
				if (shouldAttemptReconnect) {
					attemptReconnectAfterDelay();
				} else {
					setStatus(ConnectionStatus.ERROR);
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
		}
	}

	function attemptReconnectAfterDelay() {
		if (!state.currentLobbyId) {
			warn('Cannot reconnect: currentLobbyId not available.');
			setStatus(ConnectionStatus.ERROR);
			state.lastError = state.lastError || 'Reconnection details lost.';
			resetReconnectAttempts();
			return;
		}
		if (state.reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
			warn(
				`Max reconnect attempts (${MAX_RECONNECT_ATTEMPTS}) reached for lobby ${state.currentLobbyId}. Stopping.`
			);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = state.lastError || 'Failed to reconnect after multiple attempts.';
			resetReconnectAttempts();
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
		setStatus(ConnectionStatus.RECONNECTING);

		reconnectTimeoutId = window.setTimeout(() => {
			if (state.status === ConnectionStatus.DISCONNECTED && state.wasManuallyDisconnected) {
				info('Reconnect attempt cancelled due to manual disconnect.');
				return;
			}
			if (state.currentLobbyId) {
				_initiateConnectionSequence(state.currentLobbyId);
			} else {
				warn('Reconnect aborted during timeout: currentLobbyId became null.');
				setStatus(ConnectionStatus.ERROR);
				state.lastError = 'Lost reconnection details during backoff.';
			}
		}, delay);
	}

	async function connect(lobbyId: string): Promise<void> {
		if (!lobbyId) {
			const msg = 'WebSocket connect called without a lobbyId.';
			logError(msg);
			notificationStore.add('Cannot connect: Lobby ID is missing.', 'destructive');
			return Promise.reject(new Error(msg));
		}
		info(`Application requested WebSocket connection for lobby: ${lobbyId}`);

		state.wasManuallyDisconnected = false; // <<< MODIFIED: Reset flag for a new, user-initiated connection

		if (currentExternalConnectPromise) {
			info('New connect call while previous connection promise is pending. Rejecting previous.');
			_handleExternalPromiseRejection('New connection attempt initiated.');
		}
		if (state.socket) {
			info('New connect call while socket exists. Disconnecting previous.');
			disconnectSocketInternally(1000, 'Superseded by new connect call');
		}
		clearAllTimers();
		resetReconnectAttempts();
		setStatus(ConnectionStatus.INITIAL);
		return new Promise<void>((resolve, reject) => {
			currentExternalConnectPromise = { resolve, reject };
			_initiateConnectionSequence(lobbyId);
		});
	}

	function disconnectSocketInternally(code?: number, reason?: string) {
		if (state.socket) {
			info(`Internally closing socket. Code: ${code}, Reason: ${reason}`);
			state.socket.onclose = null;
			state.socket.onerror = null;
			state.socket.onmessage = null;
			state.socket.onopen = null;
			state.socket.close(code, reason);
			state.socket = null;
		}
	}

	function disconnect(): void {
		info('Application requested WebSocket manual disconnect.');
		state.wasManuallyDisconnected = true; // <<< MODIFIED: Set flag to indicate manual disconnect

		_handleExternalPromiseRejection('Manually disconnected by client action.');
		clearAllTimers();
		resetReconnectAttempts();
		disconnectSocketInternally(1000, 'Client initiated disconnect');
		setStatus(ConnectionStatus.DISCONNECTED);
		state.currentLobbyId = null;
		state.lastError = null;
	}

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
				_handleExternalPromiseRejection(sendErrorMsg, e);
				state.socket?.close(4001, 'Failed to send ConnectToLobby message');
			}
		} else {
			const notOpenMsg =
				'WebSocket not open when trying to send raw message (e.g. ConnectToLobby).';
			warn(notOpenMsg, 'Status:', state.status, 'ReadyState:', state.socket?.readyState);
			state.lastError = notOpenMsg;
			setStatus(ConnectionStatus.ERROR);
			_handleExternalPromiseRejection(notOpenMsg);
		}
	}

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
