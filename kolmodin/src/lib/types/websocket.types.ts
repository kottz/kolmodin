// --- Client -> Server Message Types ---

export interface ConnectPayload {
	admin_id: string;
	lobby_id: string;
}

export interface GlobalCommandPayload {
	command_name: string; // e.g., "Echo"
	data?: any; // Command-specific data
}

export interface GameSpecificCommandPayload {
	game_type_id: string;
	command_data: any; // This will be strongly typed within each game's types.ts
}

export type ClientToServerMessage =
	| { message_type: 'Connect'; payload: ConnectPayload }
	| { message_type: 'GlobalCommand'; payload: GlobalCommandPayload }
	| { message_type: 'GameSpecificCommand'; payload: GameSpecificCommandPayload }
	| { message_type: 'Heartbeat' }; // Simple heartbeat

// --- Server -> Client Message Types ---

export interface LobbyCreatedResponsePayload {
	// This is an API response, but good to have typed
	lobby_id: string;
	admin_id: string;
	game_type_id: string;
	twitch_channel_subscribed: string | null;
}

export interface ConnectionAckPayload {
	message: string;
	// Potentially other initial lobby info if needed on WS connect ACK
}

export interface GlobalEventPayload {
	event_name: string; // e.g., "TwitchStatusUpdate", "PlayerJoinedLobby" (if global)
	data: any;
}

export interface GameSpecificEventPayload {
	game_type_id: string;
	event_data: any; // This will be strongly typed within each game's types.ts
}

export interface SystemErrorPayload {
	message: string;
	code?: string | number; // Optional error code
}

export interface TwitchMessageRelayPayload {
	channel: string;
	sender: string;
	text: string;
	timestamp: string; // ISO string
}

export type ServerToClientMessage =
	| { message_type: 'ConnectionAck'; payload: ConnectionAckPayload }
	| { message_type: 'GlobalEvent'; payload: GlobalEventPayload }
	| { message_type: 'GameSpecificEvent'; payload: GameSpecificEventPayload }
	| { message_type: 'SystemError'; payload: SystemErrorPayload }
	| { message_type: 'TwitchMessageRelay'; payload: TwitchMessageRelayPayload }
	| { message_type: 'Pong' }; // Response to Heartbeat
