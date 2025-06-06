import type { AvailableGame, LobbyDetails } from '$lib/types/general.types';
// LobbyDetails from server matches our client-side LobbyDetails, so direct use is fine.
import { apiClient, API_ENDPOINTS } from './api.client';
import { info } from '$lib/utils/logger';

// Define the request payload type for creating a lobby, matching Rust's CreateLobbyRequest
interface CreateLobbyRequestPayload {
	game_type: string | null; // Corresponds to Option<String>
	twitch_channel: string | null; // Corresponds to Option<String>
}

export const lobbyService = {
	async fetchAvailableGames(): Promise<AvailableGame[]> {
		// ** IMPORTANT: This endpoint is NOT defined in your provided Rust code. **
		// If it doesn't exist, this will fail.
		// For now, let's return a mock list. Replace with actual API call when available.
		info('lobbyService.fetchAvailableGames: Using MOCK data.');
		return Promise.resolve([
			{
				id: 'DealNoDeal',
				name: 'Deal or No Deal',
				description: 'Beat the banker in this classic TV show game.'
			},
			{
				id: 'MedAndraOrd',
				name: 'Med Andra Ord',
				description:
					'Swedish word guessing game where the admin describes words and players guess them in chat'
			}
			// Add other game types your server supports by their game_type_id
		]);
		// Actual call would be:
		// return apiClient.get<AvailableGame[]>(API_ENDPOINTS.GET_AVAILABLE_GAMES);
	},

	async createLobby(
		gameTypeId: string, // This is the game_type selected by the user
		twitchChannel: string | null
	): Promise<LobbyDetails> {
		const payload: CreateLobbyRequestPayload = {
			game_type: gameTypeId, // Pass the selected game_type_id
			twitch_channel: twitchChannel
		};
		info('lobbyService.createLobby: Sending payload:', payload);

		// The server directly returns LobbyDetails structure on success
		const createdLobbyDetails = await apiClient.post<LobbyDetails, CreateLobbyRequestPayload>(
			API_ENDPOINTS.CREATE_LOBBY,
			payload
		);
		return createdLobbyDetails; // No transformation needed if server response matches client type
	}
};
