export interface AvailableGame {
	id: string; // This will be the game_type_id
	name: string;
	description?: string;
}

export interface LobbyDetails {
	lobby_id: string;
	admin_id: string; // The ID for the admin to connect via WebSocket
	game_type_created: string;
	twitch_channel_subscribed: string | null; // Actual channel server connected to
}

// Standardized API error structure
export interface ApiErrorResponse {
	error: string; // A general error category or code
	message: string; // User-friendly message
	details?: unknown; // Optional additional details
}
