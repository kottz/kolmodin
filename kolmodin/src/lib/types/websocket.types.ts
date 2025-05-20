// src/lib/types/websocket.types.ts

// --- Client -> Server Message Types ---

// This payload is for the NEW ConnectToLobby message
export interface ConnectToLobbyPayload {
	lobby_id: string; // This is the lobby_id obtained from /api/create-lobby
}

// Old ConnectPayload is no longer needed if ConnectToLobby replaces it.
// export interface ConnectPayload {
// 	admin_id: string;
// 	lobby_id: string;
// }

export interface GlobalCommandPayload {
	command_name: string;
	data?: any;
}

export interface GameSpecificCommandPayload {
	game_type_id: string;
	command_data: any;
}

// Updated ClientToServerMessage
export type ClientToServerMessage =
	| { messageType: 'ConnectToLobby'; payload: ConnectToLobbyPayload } // Changed from 'Connect'
	| { messageType: 'GlobalCommand'; payload: GlobalCommandPayload }
	| { messageType: 'GameSpecificCommand'; payload: GameSpecificCommandPayload }
	| { messageType: 'Heartbeat' }; // No payload for simple heartbeat

// --- Server -> Client Message Types (ensure these match your Rust server's ServerToClientMessage) ---
// These generally look okay but double-check field names if you used rename_all = "camelCase" on server.
// Your Rust ServerToClientMessage uses "messageType" and "payload" as tag/content.

export interface ConnectionAckPayload {
	// Server might send this after successful internal Connect processing
	message: string;
	// Potentially other initial lobby info if needed on WS connect ACK
}

export interface GlobalEventPayload {
	event_name: string;
	data: any;
}

export interface GameSpecificEventPayload {
	game_type_id: string;
	event_data: any;
}

export interface SystemErrorPayload {
	message: string;
	code?: string | number;
}

export interface TwitchMessageRelayPayload {
	channel: string;
	sender: string;
	text: string;
	timestamp?: string; // Server doesn't seem to send timestamp for this, but game_logic.handle_twitch_message does
}

export type ServerToClientMessage =
	| { messageType: 'ConnectionAck'; payload: ConnectionAckPayload }
	| { messageType: 'GlobalEvent'; payload: GlobalEventPayload }
	| { messageType: 'GameSpecificEvent'; payload: GameSpecificEventPayload }
	| { messageType: 'SystemError'; payload: SystemErrorPayload }
	| { messageType: 'TwitchMessageRelay'; payload: TwitchMessageRelayPayload }
	| { messageType: 'Pong' }; // If server sends explicit Pong message_type
