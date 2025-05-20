<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card';
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { lobbyStore } from '$lib/stores/lobby.store.svelte';
	import { websocketStore, ConnectionStatus } from '$lib/stores/websocket.store.svelte'; // Import ConnectionStatus
	import { notificationStore } from '$lib/stores/notification.store.svelte';
	import { lobbyService } from '$lib/services/lobby.service';
	import type { AvailableGame, LobbyDetails } from '$lib/types/general.types';
	import { isApiError } from '$lib/services/api.client';
	import { info, warn, error as logError } from '$lib/utils/logger';
	import { Loader2 } from 'lucide-svelte';

	let availableGames = $state<AvailableGame[]>([]);
	let selectedGame = $state<AvailableGame | null>(null);
	let twitchChannelLocalInput = $state('');
	let isLoadingGames = $state(true);
	let isProcessingCreation = $state(false);

	$effect(() => {
		async function loadGames() {
			info('SelectGameScreen: Fetching available games...');
			try {
				availableGames = await lobbyService.fetchAvailableGames();
			} catch (err) {
				warn('SelectGameScreen: Failed to load available games.', err);
				notificationStore.add('Could not fetch game list. Please try again.', 'destructive');
				availableGames = [];
			} finally {
				isLoadingGames = false;
			}
		}
		if (isLoadingGames) loadGames();
	});

	async function handleCreateLobby(): Promise<void> {
		if (!selectedGame || isProcessingCreation) {
			if (!selectedGame) notificationStore.add('Please select a game type.', 'warning');
			return;
		}

		isProcessingCreation = true;
		info(`SelectGameScreen: Creating lobby for "${selectedGame.name}"...`);

		try {
			// Step 1: Create lobby via HTTP
			const lobbyDetailsFromApi: LobbyDetails = await lobbyService.createLobby(
				selectedGame.id,
				twitchChannelLocalInput.trim() || null
			);
			info('SelectGameScreen: API - Lobby created:', lobbyDetailsFromApi);

			// Step 2: Update lobbyStore with details from API.
			// This is still good for other parts of the app to react to lobby state.
			lobbyStore.setLobbyDetails(lobbyDetailsFromApi);
			notificationStore.add(
				`Lobby for "${selectedGame.name}" created! Connecting...`,
				'info',
				4000
			);

			// Step 3: Connect to WebSocket, awaiting the promise from websocketStore
			await websocketStore.connect(lobbyDetailsFromApi.lobby_id);

			// If connect resolves, WebSocket is application-level connected.
			info('SelectGameScreen: WebSocket connected successfully.');

			// Step 4: Navigate to game screen
			// **** USE THE DIRECT VALUE FROM lobbyDetailsFromApi ****
			if (lobbyDetailsFromApi.game_type_created) {
				info(
					`SelectGameScreen: Navigating to game screen for type: ${lobbyDetailsFromApi.game_type_created}`
				);
				uiStore.navigateToGameActive(lobbyDetailsFromApi.game_type_created);
			} else {
				// This case implies an issue with the API response itself if game_type_created is missing
				logError(
					'SelectGameScreen: Critical - game_type_created is missing in API response. Cannot navigate.',
					lobbyDetailsFromApi
				);
				notificationStore.add(
					'Internal error: Game type missing from lobby creation response.',
					'destructive'
				);
				lobbyStore.cleanupLobbyState(true); // Attempt cleanup
			}
		} catch (err) {
			// ... (error handling remains the same) ...
			warn('SelectGameScreen: Error during lobby creation or WebSocket connection.', err);
			let errorMessage = 'Failed to start the game session.';
			if (isApiError(err)) {
				errorMessage = `Lobby creation error: ${err.message || 'Failed to create lobby.'}`;
			} else if (err instanceof Error) {
				errorMessage = `Connection error: ${err.message || 'Could not connect to game server.'}`;
			}
			notificationStore.add(errorMessage, 'destructive');

			if (lobbyStore.state.isLobbyActive) {
				info('SelectGameScreen (error path): Cleaning up lobby state.');
				lobbyStore.cleanupLobbyState(true);
			}
		} finally {
			isProcessingCreation = false;
		}
	}

	function handleGameSelection(game: AvailableGame): void {
		selectedGame = game;
	}

	function handleBack(): void {
		if (!isProcessingCreation) uiStore.navigateToHome();
	}
</script>

// src/lib/components/screens/SelectGameScreen.svelte
<!-- TEMPLATE REMAINS THE SAME -->
<div class="flex min-h-[calc(100vh-4rem)] flex-col items-center justify-center p-4 sm:p-6 md:p-8">
	<Card class="w-full max-w-lg">
		<CardHeader>
			<CardTitle class="text-2xl">Create New Lobby</CardTitle>
			<CardDescription>Select a game and optionally specify a Twitch channel.</CardDescription>
		</CardHeader>
		<CardContent class="space-y-6">
			<div>
				<h3 class="text-foreground mb-2 text-lg font-medium">Select Game Type:</h3>
				{#if isLoadingGames}
					<div
						class="text-muted-foreground flex items-center justify-center rounded-md border border-dashed p-8"
					>
						<Loader2 class="mr-2 h-6 w-6 animate-spin" />
						Loading available games...
					</div>
				{:else if availableGames.length === 0}
					<div
						class="border-destructive/50 bg-destructive/10 text-destructive-foreground rounded-md border p-4 text-center"
					>
						No game types available or failed to load.
					</div>
				{:else}
					<div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
						{#each availableGames as game (game.id)}
							<Button
								variant={selectedGame?.id === game.id ? 'default' : 'outline'}
								class="h-auto justify-start p-4 text-left {selectedGame?.id === game.id
									? 'ring-primary dark:ring-offset-background ring-2 ring-offset-2'
									: ''}"
								onclick={() => handleGameSelection(game)}
								aria-pressed={selectedGame?.id === game.id}
								disabled={isProcessingCreation}
							>
								<div class="flex flex-col">
									<span class="font-semibold">{game.name}</span>
									{#if game.description}
										<span class="text-muted-foreground mt-1 text-xs">{game.description}</span>
									{/if}
								</div>
							</Button>
						{/each}
					</div>
				{/if}
			</div>

			<div>
				<label for="twitch-channel" class="text-foreground mb-1 block text-sm font-medium"
					>Twitch Channel <span class="text-muted-foreground text-xs">(Optional)</span></label
				>
				<Input
					id="twitch-channel"
					type="text"
					bind:value={twitchChannelLocalInput}
					placeholder="your_twitch_channel_name"
					disabled={isProcessingCreation || isLoadingGames}
					class="w-full"
				/>
				<p class="text-muted-foreground mt-1 text-xs">
					If provided, the game will attempt to connect to this Twitch chat.
				</p>
			</div>

			<div class="flex flex-col space-y-3 sm:flex-row sm:space-y-0 sm:space-x-3">
				<Button
					onclick={handleBack}
					variant="outline"
					class="w-full sm:w-auto"
					disabled={isProcessingCreation}
				>
					Back
				</Button>
				<Button
					onclick={handleCreateLobby}
					class="w-full sm:flex-1"
					disabled={!selectedGame || isLoadingGames || isProcessingCreation}
				>
					{#if isProcessingCreation}
						<Loader2 class="mr-2 h-5 w-5 animate-spin" />
						Creating & Connecting...
					{:else}
						Create Lobby
					{/if}
				</Button>
			</div>
		</CardContent>
	</Card>
</div>
