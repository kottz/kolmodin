<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card'; // Assuming path for Card components
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { lobbyStore } from '$lib/stores/lobby.store.svelte';
	import { websocketStore } from '$lib/stores/websocket.store.svelte';
	import { notificationStore } from '$lib/stores/notification.store.svelte';
	import { lobbyService } from '$lib/services/lobby.service';
	import type { AvailableGame, LobbyDetails, ApiErrorResponse } from '$lib/types/general.types';
	import { isApiError } from '$lib/services/api.client'; // Import the type guard
	import { info, warn, error as logError } from '$lib/utils/logger';
	import { Loader2 } from 'lucide-svelte'; // For loading spinner

	let availableGames = $state<AvailableGame[]>([]);
	let selectedGame = $state<AvailableGame | null>(null);
	let twitchChannelLocalInput = $state(''); // Local state for the input field
	let isLoadingGames = $state(true);
	let isCreatingLobby = $state(false);

	// Fetch available games when the component mounts
	$effect(() => {
		async function loadGames() {
			info('SelectGameScreen: Fetching available games...');
			isLoadingGames = true;
			try {
				availableGames = await lobbyService.fetchAvailableGames();
				if (availableGames.length > 0) {
					// Optionally pre-select the first game
					// selectedGame = availableGames[0];
				}
			} catch (err) {
				warn('SelectGameScreen: Failed to load available games.', err);
				if (isApiError(err)) {
					notificationStore.add(`Error loading games: ${err.message}`, 'destructive');
				} else {
					notificationStore.add('Could not fetch game list. Please try again.', 'destructive');
				}
				availableGames = []; // Ensure it's an empty array on error
			} finally {
				isLoadingGames = false;
			}
		}
		loadGames();
	});

	async function handleCreateLobby(): Promise<void> {
		if (!selectedGame) {
			notificationStore.add('Please select a game type.', 'warning');
			return;
		}
		if (isCreatingLobby) return;

		isCreatingLobby = true;
		info(
			`SelectGameScreen: Attempting to create lobby for game "${selectedGame.name}" with Twitch channel "${twitchChannelLocalInput || 'None'}".`
		);

		try {
			const lobbyDetails: LobbyDetails = await lobbyService.createLobby(
				selectedGame.id,
				twitchChannelLocalInput.trim() || null
			);
			info('SelectGameScreen: Lobby created successfully via API:', lobbyDetails);

			// Now connect to WebSocket
			// The websocketStore.connect will set its status, and we can react to it.
			// For a robust flow, we might want to await a successful connection confirmation
			// from websocketStore before navigating, or handle connection failures gracefully.
			// For now, we initiate connection and navigation.
			lobbyStore.setLobbyDetails(lobbyDetails);
			websocketStore.connect(lobbyDetails.lobby_id);

			// At this point, websocketStore.state.status should be CONNECTING or CONNECTED
			// We'll optimistically navigate. If WS connection fails, websocketStore/lobbyStore
			// should reset UIStore.

			// Important: Pass the *confirmed* subscribed Twitch channel from API to lobbyStore
			uiStore.navigateToGameActive(lobbyDetails.game_type_id);
			notificationStore.add(`Lobby for "${selectedGame.name}" created!`, 'success');
		} catch (err) {
			warn('SelectGameScreen: Failed to create lobby.', err);
			let errorMessage = 'Failed to create lobby. Please try again.';
			if (isApiError(err)) {
				errorMessage = `Error: ${err.message}`;
			} else if (err instanceof Error) {
				errorMessage = err.message;
			}
			notificationStore.add(errorMessage, 'destructive');
		} finally {
			isCreatingLobby = false;
		}
	}

	function handleGameSelection(game: AvailableGame): void {
		selectedGame = game;
	}

	function handleBack(): void {
		uiStore.navigateToHome();
	}
</script>

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
					disabled={isCreatingLobby}
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
					disabled={isCreatingLobby}
				>
					Back
				</Button>
				<Button
					onclick={handleCreateLobby}
					class="w-full sm:flex-1"
					disabled={!selectedGame || isLoadingGames || isCreatingLobby}
				>
					{#if isCreatingLobby}
						<Loader2 class="mr-2 h-5 w-5 animate-spin" />
						Creating Lobby...
					{:else}
						Create Lobby
					{/if}
				</Button>
			</div>
		</CardContent>
	</Card>
</div>
