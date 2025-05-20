import { PUBLIC_WS_BASE_URL } from '$env/static/public';
import type {
	ClientToServerMessage,
	ServerToClientMessage,
	GameSpecificEventPayload,
	SystemErrorPayload
} from '$lib/types/websocket.types';
import { info, warn, error as logError, debug } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';
import { lobbyStore } from './lobby.store.svelte';
import { notificationStore } from './notification.store.svelte';

export enum ConnectionStatus {
	INITIAL = 'INITIAL',
	CONNECTING = 'CONNECTING',
	AWAITING_CONNECT_ACK = 'AWAITING_CONNECT_ACK', // New state: socket open, sent Connect, waiting for server ack
	CONNECTED = 'CONNECTED', // Server has acknowledged the Connect message
	DISCONNECTED = 'DISCONNECTED',
	RECONNECTING = 'RECONNECTING',
	ERROR = 'ERROR'
}

interface WebSocketStoreState {
	status: ConnectionStatus;
	lastError: string | null;
	socket: WebSocket | null;
	reconnectAttempts: number;
	currentPlayerId: string | null; // This will be the admin_id (player_id of admin)
}

const MAX_RECONNECT_ATTEMPTS = 5;
const INITIAL_RECONNECT_DELAY = 1000;
const MAX_RECONNECT_DELAY = 30000;
const HEARTBEAT_INTERVAL = 25000; // Server pings every 30s, client can send binary heartbeats
const HEARTBEAT_BYTE_CLIENT_SEND = 0x42; // Client sends 0x42, server echoes it back

function createWebSocketStore() {
	const state = $state<WebSocketStoreState>({
		status: ConnectionStatus.INITIAL,
		lastError: null,
		socket: null,
		reconnectAttempts: 0,
		currentPlayerId: null // Store the ID used for the Connect message
	});

	let reconnectTimeoutId: number | undefined;
	// Heartbeat: The server pings, client responds with pong. Client can also send binary heartbeats.
	// No explicit client-side interval needed for server ping/client pong.
	// For client-initiated binary heartbeat (if desired for specific proxies/load balancers):
	let clientHeartbeatIntervalId: number | undefined;

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

	function clearTimers() {
		if (reconnectTimeoutId) clearTimeout(reconnectTimeoutId);
		if (clientHeartbeatIntervalId) clearInterval(clientHeartbeatIntervalId); // Clear client heartbeat
		reconnectTimeoutId = undefined;
		clientHeartbeatIntervalId = undefined;
	}

	// Optional: Client-initiated binary heartbeat (server echoes this)
	function startClientHeartbeat() {
		debug('Starting client-initiated binary heartbeat...');
		stopClientHeartbeat();
		clientHeartbeatIntervalId = window.setInterval(() => {
			if (
				state.socket &&
				state.socket.readyState === WebSocket.OPEN &&
				state.status === ConnectionStatus.CONNECTED
			) {
				debug('Client sending binary heartbeat (0x42)');
				try {
					state.socket.send(new Uint8Array([HEARTBEAT_BYTE_CLIENT_SEND]));
				} catch (e) {
					logError('Failed to send client binary heartbeat:', e);
				}
			}
		}, HEARTBEAT_INTERVAL); // Use same interval as server pings or adjust
	}

	function stopClientHeartbeat() {
		debug('Stopping client-initiated binary heartbeat.');
		if (clientHeartbeatIntervalId) clearInterval(clientHeartbeatIntervalId);
		clientHeartbeatIntervalId = undefined;
	}

	function _connect(playerIdToConnect: string) {
		// Renamed from adminId for clarity, this is the ID used in Connect msg
		if (
			state.status === ConnectionStatus.CONNECTING ||
			state.status === ConnectionStatus.RECONNECTING ||
			state.status === ConnectionStatus.AWAITING_CONNECT_ACK ||
			(state.status === ConnectionStatus.CONNECTED && state.currentPlayerId === playerIdToConnect) // Already connected with this ID
		) {
			info(
				`WebSocket connection attempt for player ${playerIdToConnect} already in progress or completed with this ID.`
			);
			return;
		}

		clearTimers();
		state.currentPlayerId = playerIdToConnect; // Store the ID we will send in Connect message

		const wsUrl = `${PUBLIC_WS_BASE_URL}/ws`; // Generic WebSocket endpoint
		info(`Attempting to connect to WebSocket: ${wsUrl} for player ${playerIdToConnect}`);
		setStatus(
			state.reconnectAttempts > 0 ? ConnectionStatus.RECONNECTING : ConnectionStatus.CONNECTING
		);
		state.lastError = null;

		try {
			const newSocket = new WebSocket(wsUrl);
			state.socket = newSocket;

			newSocket.onopen = () => {
				info(
					`WebSocket underlying connection open for player ${playerIdToConnect}. Sending Connect message...`
				);
				// DO NOT set to CONNECTED yet. Wait for server's response to our Connect message.
				setStatus(ConnectionStatus.AWAITING_CONNECT_ACK);
				state.lastError = null; // Clear error on new attempt

				send({
					message_type: 'Connect',
					payload: { player_id: playerIdToConnect } // Use the provided playerId (admin_id from createLobby)
				});
				// Heartbeat starts after server acknowledges Connect (e.g. via ConnectionAck or first GameState)
			};

			newSocket.onmessage = (event: MessageEvent) => {
				try {
					// Handle binary heartbeat echo from server
					if (event.data instanceof ArrayBuffer || event.data instanceof Blob) {
						const reader = new FileReader();
						reader.onload = function () {
							const arrayBuffer = this.result as ArrayBuffer;
							const byteArray = new Uint8Array(arrayBuffer);
							if (byteArray.length === 1 && byteArray[0] === HEARTBEAT_BYTE_CLIENT_SEND) {
								debug('Client received binary heartbeat echo from server.');
							} else {
								warn('Client received unexpected binary message:', byteArray);
							}
						};
						if (event.data instanceof Blob) {
							reader.readAsArrayBuffer(event.data);
						} else if (event.data instanceof ArrayBuffer) {
							// Directly process ArrayBuffer if not needing FileReader
							const byteArray = new Uint8Array(event.data);
							if (byteArray.length === 1 && byteArray[0] === HEARTBEAT_BYTE_CLIENT_SEND) {
								debug('Client received binary heartbeat echo from server (ArrayBuffer).');
							} else {
								warn('Client received unexpected binary message (ArrayBuffer):', byteArray);
							}
						}
						return;
					}

					const message = JSON.parse(event.data as string) as ServerToClientMessage;
					debug('WebSocket message received:', message);

					// If we were waiting for Connect ACK and get any valid app message,
					// consider the connection fully established.
					if (state.status === ConnectionStatus.AWAITING_CONNECT_ACK) {
						if (message.message_type !== 'SystemError') {
							// Unless it's an error related to Connect
							info(
								'WebSocket application-level connection confirmed (received first valid message post-Connect).'
							);
							setStatus(ConnectionStatus.CONNECTED);
							resetReconnectAttempts(); // Reset on successful logical connection
							startClientHeartbeat(); // Start client-side binary heartbeat if desired
							// Server also pings, so this client one is optional
						}
					}

					switch (message.message_type) {
						// Pong from server to client's ping is handled by browser/WS library typically
						// No explicit 'Pong' message_type needed from our app protocol if server handles native pings
						case 'GameSpecificEvent':
							gameEventRouter.routeGameSpecificEvent(message.payload as GameSpecificEventPayload);
							break;
						case 'ConnectionAck': // Server might send this after successful internal Connect processing
							// This is where we'd definitively move to CONNECTED if we haven't already
							if (state.status !== ConnectionStatus.CONNECTED) {
								info('ConnectionAck received. Finalizing connection.');
								setStatus(ConnectionStatus.CONNECTED);
								resetReconnectAttempts();
								startClientHeartbeat();
							}
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
							lobbyStore.handleSystemError(message.payload as SystemErrorPayload);
							if (state.status === ConnectionStatus.AWAITING_CONNECT_ACK) {
								// If error occurs while waiting for connect ack, connection failed
								setStatus(ConnectionStatus.ERROR);
								state.lastError = message.payload.message;
								state.socket?.close(4002, 'Connect message processing failed'); // Close socket
							}
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
				logError('WebSocket error event occurred. See onclose for status update.', event);
				state.lastError = state.lastError || 'WebSocket connection error.';
				// `onclose` will usually follow and handle state transition + reconnection.
			};

			newSocket.onclose = (event: CloseEvent) => {
				info(
					`WebSocket connection closed. Code: ${event.code}, Reason: '${event.reason}', Clean: ${event.wasClean}`
				);
				const statusBeforeClose = state.status;
				stopClientHeartbeat();
				state.socket = null;

				if (statusBeforeClose === ConnectionStatus.DISCONNECTED) {
					info('WebSocket was manually disconnected.');
					// setStatus(ConnectionStatus.DISCONNECTED) was already called.
					resetReconnectAttempts();
					return;
				}

				state.lastError =
					state.lastError ||
					`Connection closed (Code: ${event.code}${event.reason ? ` - Reason: ${event.reason}` : ''})`;

				if (event.code !== 1000 || statusBeforeClose === ConnectionStatus.AWAITING_CONNECT_ACK) {
					// Abnormal closure, or closed by server while we were trying to logically connect
					attemptReconnect();
				} else {
					// Clean closure (code 1000)
					if (statusBeforeClose === ConnectionStatus.CONNECTED) {
						notificationStore.add('Connection cleanly closed by server.', 'info');
						setStatus(ConnectionStatus.ERROR); // Session ended by server
					} else {
						// This case should be less likely if AWAITING_CONNECT_ACK + close leads to attemptReconnect
						setStatus(ConnectionStatus.ERROR);
						state.lastError =
							state.lastError || 'Server cleanly closed connection during an unexpected state.';
					}
					resetReconnectAttempts();
				}
			};
		} catch (err) {
			logError('Failed to create WebSocket instance:', err);
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Failed to initialize WebSocket connection.';
			state.socket = null;
			if (state.currentPlayerId) attemptReconnect(); // Only if we had a player ID to try with
		}
	}

	function attemptReconnect() {
		if (!state.currentPlayerId) {
			// Changed from lastAdminId/lastLobbyId
			warn('Cannot reconnect: currentPlayerId for Connect message not available.');
			setStatus(ConnectionStatus.ERROR);
			state.lastError = 'Reconnection details (currentPlayerId) lost.';
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
			`Attempting reconnect ${state.reconnectAttempts}/${MAX_RECONNECT_ATTEMPTS} for player ${state.currentPlayerId} in ${delay / 1000}s...`
		);
		setStatus(ConnectionStatus.RECONNECTING);

		reconnectTimeoutId = window.setTimeout(() => {
			if (state.status === ConnectionStatus.DISCONNECTED) {
				info('Reconnect attempt cancelled due to manual disconnect.');
				return;
			}
			if (state.currentPlayerId) {
				// Re-check, should be set
				_connect(state.currentPlayerId);
			} else {
				warn('Reconnect aborted during timeout: currentPlayerId became null.');
				setStatus(ConnectionStatus.ERROR);
				state.lastError = 'Lost reconnection details during backoff (currentPlayerId).';
			}
		}, delay);
	}

	// This 'playerId' is the admin_id from createLobby response
	function connect(playerId: string): void {
		if (!playerId) {
			logError('WebSocket connect called without a playerId.');
			notificationStore.add('Cannot connect: Player ID is missing.', 'destructive');
			return;
		}
		info(`Application requested WebSocket connection for player: ${playerId}`);
		clearTimers();
		resetReconnectAttempts();

		if (state.socket) {
			info('An existing WebSocket is present. Closing it before new connect.');
			state.socket.onclose = null;
			state.socket.onerror = null;
			state.socket.onopen = null;
			state.socket.onmessage = null;
			state.socket.close(1000, 'Client initiating new connection');
			state.socket = null;
		}
		setStatus(ConnectionStatus.INITIAL);
		_connect(playerId);
	}

	function disconnect(): void {
		info('Application requested WebSocket disconnect.');
		clearTimers();
		resetReconnectAttempts();
		const oldStatus = state.status;

		if (state.socket) {
			setStatus(ConnectionStatus.DISCONNECTED); // Set BEFORE closing
			state.socket.close(1000, 'Client initiated disconnect');
		} else {
			if (oldStatus !== ConnectionStatus.DISCONNECTED) {
				setStatus(ConnectionStatus.DISCONNECTED);
			}
		}
		state.currentPlayerId = null; // Clear current player ID on manual disconnect
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
