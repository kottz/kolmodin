import { PUBLIC_WS_BASE_URL } from '$env/static/public';
import type {
	ClientToServerMessage,
	ServerToClientMessage,
	GameSpecificEventPayload,
	SystemErrorPayload
} from '$lib/types/websocket.types'; // Added SystemErrorPayload
import { info, warn, error as logError, debug } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';
import { lobbyStore } from './lobby.store.svelte';
import { notificationStore } from './notification.store.svelte';

export enum ConnectionStatus {
	INITIAL = 'INITIAL',
	CONNECTING = 'CONNECTING',
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
	lastAdminId: string | null;
	lastLobbyId: string | null;
}

const MAX_RECONNECT_ATTEMPTS = 5;
const INITIAL_RECONNECT_DELAY = 1000;
const MAX_RECONNECT_DELAY = 30000;
const HEARTBEAT_INTERVAL = 25000;
const HEARTBEAT_TIMEOUT = 5000;

function createWebSocketStore() {
	const state = $state<WebSocketStoreState>({
		status: ConnectionStatus.INITIAL,
		lastError: null,
		socket: null,
		reconnectAttempts: 0,
		lastAdminId: null,
		lastLobbyId: null
	});

	let reconnectTimeoutId: number | undefined;
	let heartbeatIntervalId: number | undefined;
	let heartbeatTimeoutId: number | undefined;

	// Helper to update status and notify lobbyStore
	function setStatus(newStatus: ConnectionStatus) {
		const previousStatus = state.status;
		if (previousStatus !== newStatus) {
			state.status = newStatus;
			// Notify lobbyStore of the status change
			lobbyStore.handleWebSocketStatusChange(newStatus, previousStatus);
			debug(`WebSocket status changed from ${previousStatus} -> ${newStatus}`);
		} else {
			debug(`WebSocket status unchanged: ${newStatus}`);
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
		if (heartbeatIntervalId) clearInterval(heartbeatIntervalId);
		if (heartbeatTimeoutId) clearTimeout(heartbeatTimeoutId);
		reconnectTimeoutId = undefined;
		heartbeatIntervalId = undefined;
		heartbeatTimeoutId = undefined;
	}

	function startHeartbeat() {
		debug('Starting heartbeat...');
		stopHeartbeat();

		heartbeatIntervalId = window.setInterval(() => {
			if (state.socket && state.socket.readyState === WebSocket.OPEN) {
				debug('Sending Heartbeat');
				state.socket.send(JSON.stringify({ message_type: 'Heartbeat' }));

				if (heartbeatTimeoutId) clearTimeout(heartbeatTimeoutId);
				heartbeatTimeoutId = window.setTimeout(() => {
					warn('Heartbeat timeout: No Pong received.');
					state.lastError = 'Connection lost (heartbeat timeout)';
					state.socket?.close(4001, 'Heartbeat Timeout'); // Triggers onclose
				}, HEARTBEAT_TIMEOUT);
			} else {
				warn('Cannot send heartbeat, socket not open or state invalid.');
			}
		}, HEARTBEAT_INTERVAL);
	}

	function stopHeartbeat() {
		debug('Stopping heartbeat.');
		if (heartbeatIntervalId) clearInterval(heartbeatIntervalId);
		if (heartbeatTimeoutId) clearTimeout(heartbeatTimeoutId);
		heartbeatIntervalId = undefined;
		heartbeatTimeoutId = undefined;
	}

	function _connect(adminId: string, lobbyId: string) {
		if (
			state.status === ConnectionStatus.CONNECTED &&
			state.socket?.readyState === WebSocket.OPEN
		) {
			// If already connected to the same lobby, potentially re-send Connect or just info.
			if (state.lastLobbyId === lobbyId && state.lastAdminId === adminId) {
				info('WebSocket already open and connected to the same lobby.');
				// Optionally re-send connect if server expects it on page refresh with existing socket
				// send({ message_type: 'Connect', payload: { admin_id: adminId, lobby_id: lobbyId }});
				return;
			}
			// If connected to a different lobby, disconnect first is handled by `connect`
		}

		if (
			state.status === ConnectionStatus.CONNECTING ||
			state.status === ConnectionStatus.RECONNECTING
		) {
			info('WebSocket connection attempt already in progress for this or another lobby.');
			return;
		}

		clearTimers();

		state.lastAdminId = adminId;
		state.lastLobbyId = lobbyId;

		const wsUrl = `${PUBLIC_WS_BASE_URL}/ws/${lobbyId}`;
		info(`Attempting to connect to WebSocket: ${wsUrl}`);
		setStatus(
			state.reconnectAttempts > 0 ? ConnectionStatus.RECONNECTING : ConnectionStatus.CONNECTING
		);
		state.lastError = null;

		try {
			const newSocket = new WebSocket(wsUrl);
			state.socket = newSocket; // Assign new socket to state

			newSocket.onopen = () => {
				info('WebSocket connection established.');
				setStatus(ConnectionStatus.CONNECTED);
				state.lastError = null;
				resetReconnectAttempts(); // Crucial: reset attempts on successful open
				startHeartbeat();

				send({
					message_type: 'Connect',
					payload: { admin_id: adminId, lobby_id: lobbyId }
				});
			};

			newSocket.onmessage = (event: MessageEvent) => {
				if (heartbeatTimeoutId) clearTimeout(heartbeatTimeoutId); // Clear pong timeout on ANY message
				heartbeatTimeoutId = undefined;

				try {
					const message = JSON.parse(event.data as string) as ServerToClientMessage;
					debug('WebSocket message received:', message);

					switch (message.message_type) {
						case 'Pong':
							debug('Pong received');
							break;
						case 'GameSpecificEvent':
							gameEventRouter.routeGameSpecificEvent(message.payload as GameSpecificEventPayload);
							break;
						case 'ConnectionAck':
							lobbyStore.handleConnectionAck(message.payload);
							break;
						case 'GlobalEvent':
							lobbyStore.handleGlobalEvent(message.payload);
							break;
						case 'SystemError':
							logError(
								'WebSocket Store: Received SystemError from server:',
								message.payload.message
							);
							lobbyStore.handleSystemError(message.payload as SystemErrorPayload); // Ensure type
							break;
						case 'TwitchMessageRelay':
							lobbyStore.handleTwitchMessageRelay(message.payload);
							break;
						default:
							warn(
								`WebSocket Store: Received unhandled message type: ${(message as any).message_type}`
							);
							notificationStore.add(
								`Received unknown server message: ${(message as any).message_type}`,
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
				// Browsers often fire 'error' then 'close'. 'onclose' handles status and reconnection.
				logError('WebSocket error event occurred. Details should follow in onclose.', event);
				state.lastError = state.lastError || 'WebSocket connection error occurred.';
			};

			newSocket.onclose = (event: CloseEvent) => {
				info(
					`WebSocket connection closed. Code: ${event.code}, Reason: '${event.reason}', Clean: ${event.wasClean}`
				);
				const previousStatusBeforeCloseLogic = state.status; // Capture status before it's changed below
				stopHeartbeat();
				state.socket = null;

				// If status was already set to DISCONNECTED (manual disconnect), do nothing more here.
				if (previousStatusBeforeCloseLogic === ConnectionStatus.DISCONNECTED) {
					info('WebSocket was manually disconnected. No further action in onclose.');
					// setStatus(ConnectionStatus.DISCONNECTED) was already called.
					resetReconnectAttempts(); // Ensure attempts are reset.
					return;
				}

				state.lastError =
					state.lastError ||
					`Connection closed (Code: ${event.code}${event.reason ? ` - Reason: ${event.reason}` : ''})`;

				if (event.code !== 1000) {
					// Abnormal closure (not a clean server close or client disconnect)
					attemptReconnect(); // This will set status to RECONNECTING or ERROR
				} else {
					// Clean closure (code 1000)
					if (previousStatusBeforeCloseLogic === ConnectionStatus.CONNECTED) {
						notificationStore.add('Connection cleanly closed by server.', 'info');
						setStatus(ConnectionStatus.ERROR); // Treat as session ended by server
					} else {
						// Cleanly closed during CONNECTING/RECONNECTING implies server rejected the attempt
						setStatus(ConnectionStatus.ERROR);
						state.lastError =
							state.lastError || 'Server cleanly closed connection during attempt (rejected).';
					}
					resetReconnectAttempts(); // No auto-reconnect for clean server closes or rejections
				}
			};
		} catch (err) {
			logError('Failed to create WebSocket instance:', err);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Failed to initialize WebSocket connection.';
			state.socket = null;
			attemptReconnect();
		}
	}

	function attemptReconnect() {
		if (!state.lastAdminId || !state.lastLobbyId) {
			warn('Cannot reconnect: adminId or lobbyId not available.');
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Reconnection details lost.';
			resetReconnectAttempts(); // Prevent further attempts
			return; // lobbyStore will be notified by setStatus(ERROR)
		}

		if (state.reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
			warn(`Max reconnect attempts (${MAX_RECONNECT_ATTEMPTS}) reached. Stopping.`);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = state.lastError || 'Failed to reconnect after multiple attempts.';
			resetReconnectAttempts();
			// lobbyStore is notified via setStatus(ERROR) and will call its cleanup
			return;
		}

		state.reconnectAttempts++;
		const delay = Math.min(
			INITIAL_RECONNECT_DELAY * Math.pow(2, state.reconnectAttempts - 1),
			MAX_RECONNECT_DELAY
		);

		info(
			`Attempting reconnect ${state.reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS} in ${delay / 1000}s...`
		);
		setStatus(ConnectionStatus.RECONNECTING);

		reconnectTimeoutId = window.setTimeout(() => {
			if (state.status === ConnectionStatus.DISCONNECTED) {
				// Check if manually disconnected during timeout
				info('Reconnect attempt cancelled due to manual disconnect.');
				return;
			}
			// Ensure IDs are still present (should be, but defensive)
			if (state.lastAdminId && state.lastLobbyId) {
				_connect(state.lastAdminId, state.lastLobbyId);
			} else {
				// Should have been caught by the initial check in attemptReconnect
				warn('Reconnect aborted: adminId or lobbyId became null unexpectedly.');
				setStatus(ConnectionStatus.ERROR);
				state.lastError = 'Lost reconnection details during backoff.';
			}
		}, delay);
	}

	function connect(adminId: string, lobbyId: string): void {
		info('Manual connect initiated by application.');
		clearTimers(); // Clear any existing reconnect or heartbeat timers
		resetReconnectAttempts();

		if (state.socket) {
			info('An existing WebSocket connection/attempt is present. Closing it before new connect.');
			// To prevent the onclose of the *old* socket from triggering auto-reconnect logic for the old session:
			state.socket.onclose = null; // Detach old onclose handler
			state.socket.onerror = null; // Detach old onerror
			state.socket.onopen = null;
			state.socket.onmessage = null;
			state.socket.close(1000, 'Client initiating new connection');
			state.socket = null;
		}
		setStatus(ConnectionStatus.INITIAL); // Set status for the new attempt
		_connect(adminId, lobbyId);
	}

	function disconnect(): void {
		info('Manual disconnect initiated by application.');
		clearTimers();
		resetReconnectAttempts();
		const oldStatus = state.status;

		if (state.socket) {
			// Set status BEFORE closing to ensure onclose handler knows it's manual
			setStatus(ConnectionStatus.DISCONNECTED);
			state.socket.close(1000, 'Client initiated disconnect');
			// onclose will handle setting state.socket = null
		} else {
			// If no socket, but status wasn't DISCONNECTED, update it.
			if (oldStatus !== ConnectionStatus.DISCONNECTED) {
				setStatus(ConnectionStatus.DISCONNECTED);
			}
		}
		// Clearing lastAdminId/LobbyId here means no "reconnect to previous session" feature after manual disconnect.
		// state.lastAdminId = null;
		// state.lastLobbyId = null;
	}

	function send(message: ClientToServerMessage): void {
		if (state.socket && state.socket.readyState === WebSocket.OPEN) {
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
			warn('WebSocket not open. Cannot send message:', message);
			state.lastError = 'Connection not available to send message.';
			notificationStore.add('Cannot send message: Not connected to server.', 'warning');
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
