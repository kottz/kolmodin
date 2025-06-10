import type { LobbyDetails } from '$lib/types/general.types';
import type {
	ConnectionAckPayload,
	GlobalEventPayload,
	TwitchMessageRelayPayload,
	SystemErrorPayload
} from '$lib/types/websocket.types';
import { websocketStore, ConnectionStatus } from './websocket.store.svelte';
import { uiStore } from './ui.store.svelte';
import { notificationStore } from './notification.store.svelte'; // Corrected import
import { info, warn, debug, error as logError } from '$lib/utils/logger'; // Corrected import for error

interface LobbyStoreState {
	lobbyId: string | null;
	adminId: string | null;
	activeGameTypeId: string | null;
	subscribedTwitchChannel: string | null;
	twitchIrcStatus: string | null;
	isLobbyActive: boolean;
}

function createLobbyStore() {
	const state = $state<LobbyStoreState>({
		lobbyId: null,
		adminId: null,
		activeGameTypeId: null,
		subscribedTwitchChannel: null,
		twitchIrcStatus: 'N/A',
		isLobbyActive: false
	});

	function setLobbyDetails(details: LobbyDetails): void {
		info('LobbyStore: Setting lobby details:', details);
		state.lobbyId = details.lobby_id;
		state.adminId = details.admin_id;
		state.activeGameTypeId = details.game_type_created;
		state.subscribedTwitchChannel = details.twitch_channel_subscribed;
		state.isLobbyActive = true;
		state.twitchIrcStatus = details.twitch_channel_subscribed ? 'Connecting...' : 'N/A';
	}

	function handleConnectionAck(payload: ConnectionAckPayload): void {
		info('LobbyStore: Received ConnectionAck from server:', payload.message);
		notificationStore.add(payload.message || 'Connected to lobby!', 'success', 3000);
	}

	function handleGlobalEvent(payload: GlobalEventPayload): void {
		debug('LobbyStore: Received GlobalEvent:', payload);
		switch (payload.event_name) {
			case 'TwitchStatusUpdate':
				const statusData = payload.data as {
					channel_name?: string;
					status_type: string;
					details?: string;
				};
				let statusText = `Channel: ${statusData.channel_name || state.subscribedTwitchChannel || 'Unknown'}`;
				statusText += `, Status: ${statusData.status_type}`;
				if (statusData.details) statusText += ` (${statusData.details})`;
				state.twitchIrcStatus = statusText;
				info(`LobbyStore: Twitch IRC Status Updated - ${statusText}`);
				break;
			default:
				warn(`LobbyStore: Unhandled GlobalEvent name: ${payload.event_name}`);
		}
	}

	function handleTwitchMessageRelay(payload: TwitchMessageRelayPayload): void {
		debug(`LobbyStore: TwitchRelay: #${payload.channel} ${payload.sender}: ${payload.text}`);
		// This could potentially update another store dedicated to chat messages if needed
	}

	function handleSystemError(payload: SystemErrorPayload): void {
		logError('LobbyStore: Received SystemError from server:', payload.message);
		notificationStore.add(`Server error: ${payload.message}`, 'destructive');
		// If error indicates lobby closure, cleanup might be needed
		// e.g. if (payload.code === 'LOBBY_CLOSED') cleanupLobbyState(true);
	}

	// New method to be called by websocketStore
	function handleWebSocketStatusChange(
		newStatus: ConnectionStatus,
		previousStatus: ConnectionStatus | null
	): void {
		debug(`LobbyStore: WebSocket status changed from ${previousStatus} to ${newStatus}`);
		if (
			(newStatus === ConnectionStatus.DISCONNECTED || newStatus === ConnectionStatus.ERROR) &&
			previousStatus === ConnectionStatus.CONNECTED && // Only cleanup if we WERE connected
			state.isLobbyActive
		) {
			info(
				'LobbyStore: WebSocket disconnected/errored while lobby was active, cleaning up lobby state.'
			);
			cleanupLobbyState(newStatus === ConnectionStatus.ERROR);
		}
	}

	function cleanupLobbyState(dueToError: boolean = false): void {
		if (!state.isLobbyActive && !state.lobbyId) {
			debug('LobbyStore: cleanupLobbyState called, but no active lobby to clean.');
			// Ensure UI is reset if somehow out of sync
			if (uiStore.state.currentScreen !== 'home') {
				uiStore.resetToHomeState();
			}
			return;
		}

		info('LobbyStore: Cleaning up lobby state.');
		state.lobbyId = null;
		state.adminId = null;
		state.activeGameTypeId = null;
		state.subscribedTwitchChannel = null;
		state.twitchIrcStatus = 'N/A';
		state.isLobbyActive = false;

		uiStore.resetToHomeState();

		// If websocket is still connected or trying to connect, tell it to disconnect fully
		// as the lobby session is no longer valid from the client's perspective.
		const wsCurrentStatus = websocketStore.state.status;
		if (
			wsCurrentStatus === ConnectionStatus.CONNECTED ||
			wsCurrentStatus === ConnectionStatus.CONNECTING ||
			wsCurrentStatus === ConnectionStatus.RECONNECTING
		) {
			info('LobbyStore: Instructing WebSocket to disconnect due to lobby cleanup.');
			websocketStore.disconnect(); // Ensure a full stop for this session
		}
	}

	function userLeaveLobby(): void {
		info('LobbyStore: User initiated leave lobby.');
		// Send LeaveLobby message to server to explicitly indicate intentional disconnect
		websocketStore.send({ messageType: 'LeaveLobby' });
		cleanupLobbyState(false); // Not due to an error, triggers graceful WS disconnect
	}

	return {
		get state() {
			return state;
		},
		setLobbyDetails,
		handleConnectionAck,
		handleGlobalEvent,
		handleTwitchMessageRelay,
		handleSystemError,
		handleWebSocketStatusChange, // Expose this for websocketStore
		cleanupLobbyState,
		userLeaveLobby
	};
}

export const lobbyStore = createLobbyStore();
