<script lang="ts">
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { lobbyStore } from '$lib/stores/lobby.store.svelte';
	import { Button } from '$lib/components/ui/button'; // Assuming index.ts exports Button
	import { info, warn } from '$lib/utils/logger';

	import DealNoDealAdminView from '$lib/components/games/DealNoDeal/AdminView.svelte';
	import MedAndraOrdAdminView from '$lib/components/games/MedAndraOrd/AdminView.svelte';
	// Optional shared components (ensure they exist or comment out)
	// import GameLog from '$lib/components/shared/GameLog.svelte';
	// import LatestGameEventDisplay from '$lib/components/shared/LatestGameEventDisplay.svelte';

	const gameComponentMap = {
		DealNoDeal: DealNoDealAdminView,
		MedAndraOrd: MedAndraOrdAdminView
		// HelloWorldGame: HelloWorldAdminView,
	};

	// Define a more general type for component constructors if you have many
	// For Svelte components, `typeof import('*.svelte').default` can be more precise
	// but for simplicity here, `any` or a union of known components works.
	type GameAdminComponentConstructor = typeof DealNoDealAdminView; // | typeof HelloWorldAdminView;

	const activeGameTypeId = $derived(uiStore.state.activeGameTypeIdForUI);
	const lobbyId = $derived(lobbyStore.state.lobbyId);
	const subscribedTwitchChannel = $derived(lobbyStore.state.subscribedTwitchChannel);
	const twitchIrcStatus = $derived(lobbyStore.state.twitchIrcStatus);

	let ActiveGameComponent = $derived(getGameComponent(activeGameTypeId));

	function getGameComponent(gameTypeId: string | null): GameAdminComponentConstructor | null {
		if (!gameTypeId) {
			return null;
		}
		const component = gameComponentMap[gameTypeId as keyof typeof gameComponentMap];
		if (!component) {
			warn(`GameScreen: No AdminView component found for gameTypeId "${gameTypeId}".`);
			return null;
		}
		return component as GameAdminComponentConstructor;
	}

	function handleLeaveLobby(): void {
		info('GameScreen: "Leave Lobby" clicked.');
		lobbyStore.userLeaveLobby();
	}

	$effect(() => {
		if (uiStore.state.currentScreen === 'gameActive' && !activeGameTypeId) {
			warn(
				'GameScreen: Active game type ID became null while game screen is active. Navigating home.'
			);
			uiStore.navigateToHome(); // Or uiStore.resetToHomeState() if that's more appropriate
		}
	});
</script>

<div class="flex h-screen flex-col">
	<header class="border-border bg-background/95 sticky top-0 z-10 border-b p-3 backdrop-blur">
		<div class="container mx-auto flex items-center justify-between">
			<div>
				<h1 class="text-foreground text-xl font-semibold">
					Lobby: <span class="text-primary font-mono">{lobbyId || 'N/A'}</span>
				</h1>
				{#if subscribedTwitchChannel}
					<p class="text-muted-foreground text-xs">
						Twitch: {subscribedTwitchChannel} ({twitchIrcStatus || 'Status Unknown'})
					</p>
				{/if}
			</div>
			<!-- Ensure onclick is used for event handling with Svelte 5 components -->
			<Button onclick={handleLeaveLobby} variant="outline" size="sm">Leave Lobby</Button>
		</div>
	</header>

	<div class="container mx-auto flex-1 overflow-y-auto">
		{#if ActiveGameComponent}
			<!-- Correct Svelte 5 dynamic component rendering -->
			<ActiveGameComponent />
		{:else if activeGameTypeId}
			<div class="mt-10 flex flex-col items-center justify-center text-center">
				<p class="text-muted-foreground text-lg">
					Loading game interface for <span class="font-semibold">{activeGameTypeId}</span>...
				</p>
				<p class="text-muted-foreground mt-2 text-sm">
					If this persists, the game component might be missing or misconfigured in
					GameScreen.svelte.
				</p>
			</div>
		{:else}
			<div class="mt-10 flex flex-col items-center justify-center text-center">
				<p class="text-destructive text-lg font-semibold">
					Error: No active game specified for GameScreen.
				</p>
				<p class="text-muted-foreground mt-2 text-sm">
					This should not happen, check navigation logic.
				</p>
			</div>
		{/if}
	</div>

	<!-- Optional Footer for Logs
	<footer class="border-t border-border p-4 bg-muted/40">
		...
	</footer>
	-->
</div>
