<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { streamStore } from '$lib/stores/stream.store.svelte';
	import { info, debug } from '$lib/utils/logger';
	import type { StreamEvent } from '$lib/types/stream.types';

	// Stream-specific game view components
	import MedAndraOrdStreamView from '$lib/components/games/MedAndraOrd/StreamView.svelte';
	import DealNoDealStreamView from '$lib/components/games/DealNoDeal/StreamView.svelte';

	const state = $derived(streamStore.state);
	const displayConfig = $derived(streamStore.displayConfig);

	// Game component mapping
	const gameStreamComponentMap = {
		MedAndraOrd: MedAndraOrdStreamView,
		DealNoDeal: DealNoDealStreamView
	};

	let ActiveGameStreamComponent = $derived(getGameStreamComponent(state.currentGameType));

	function getGameStreamComponent(gameTypeId: string | null) {
		if (!gameTypeId) return null;

		const component = gameStreamComponentMap[gameTypeId as keyof typeof gameStreamComponentMap];
		return component || null;
	}

	onMount(() => {
		info('StreamWindow: Mounted, initializing stream store');
		streamStore.initialize();

		// Set document title
		document.title = 'Game Stream View';

		// Add specific styling for stream window
		document.body.classList.add('stream-window');

		return () => {
			document.body.classList.remove('stream-window');
		};
	});

	onDestroy(() => {
		info('StreamWindow: Destroying, cleaning up stream store');
		streamStore.cleanup();
	});

	function formatEventData(event: StreamEvent): string {
		if (typeof event.data === 'object') {
			return JSON.stringify(event.data, null, 2);
		}
		return String(event.data);
	}
</script>

<svelte:head>
	<title>Game Stream View</title>
	<meta name="robots" content="noindex, nofollow" />
</svelte:head>

<div
	class="stream-container min-h-screen bg-gradient-to-br from-slate-900 via-purple-900 to-slate-900"
>
	{#if !state.isVisible}
		<!-- Hidden state -->
		<div class="flex h-screen items-center justify-center">
			<div class="text-center text-white/60">
				<div class="mb-4 text-6xl">👁️</div>
				<h2 class="text-2xl font-bold">Stream Hidden</h2>
				<p class="text-lg">Waiting for admin to show stream...</p>
			</div>
		</div>
	{:else if !streamStore.hasActiveGame}
		<!-- No active game -->
		<div class="flex h-screen items-center justify-center">
			<div class="text-center text-white">
				<div class="mb-6 text-8xl">🎮</div>
				<h1 class="mb-4 text-4xl font-bold">Game Stream</h1>
				<p class="text-xl text-white/80">Waiting for game to start...</p>
				<div class="mt-8">
					<div class="mx-auto h-2 w-48 overflow-hidden rounded-full bg-white/20">
						<div class="h-full animate-pulse bg-gradient-to-r from-blue-500 to-purple-500"></div>
					</div>
				</div>
			</div>
		</div>
	{:else if !streamStore.isReady}
		<!-- Game active but no state yet -->
		<div class="flex h-screen items-center justify-center">
			<div class="text-center text-white">
				<div class="mb-6 animate-spin text-6xl">⚙️</div>
				<h2 class="mb-4 text-3xl font-bold">Loading {state.currentGameType}</h2>
				<p class="text-lg text-white/80">Setting up game view...</p>
			</div>
		</div>
	{:else}
		<!-- Active game with state -->
		<div class="relative h-screen overflow-hidden">
			{#if ActiveGameStreamComponent}
				<!-- Game-specific stream view -->
				<ActiveGameStreamComponent gameState={state.gameState} {displayConfig} />
			{:else}
				<!-- Fallback generic view for games without specific stream components -->
				<div class="p-8">
					<div class="mb-8 text-center text-white">
						<h1 class="mb-2 text-4xl font-bold">{state.currentGameType}</h1>
						<p class="text-white/80">Stream View (Generic)</p>
					</div>

					{#if state.gameState}
						<div class="mx-auto max-w-4xl rounded-lg bg-black/30 p-6 backdrop-blur-sm">
							<h3 class="mb-4 text-xl font-bold text-white">Game State</h3>
							<pre class="max-h-96 overflow-auto text-sm text-white/90">{JSON.stringify(
									state.gameState,
									null,
									2
								)}</pre>
						</div>
					{/if}
				</div>
			{/if}

			<!-- Stream Events Overlay - Disabled for clean design -->
		</div>
	{/if}
</div>

<style>
	:global(.stream-window) {
		/* Remove default margins/padding for stream window */
		margin: 0;
		padding: 0;
		/* Prevent text selection in stream window */
		user-select: none;
		/* Ensure full height */
		height: 100vh;
		overflow: hidden;
	}

	/* Animation for stream events */
	@keyframes slide-in-from-right {
		from {
			transform: translateX(100%);
			opacity: 0;
		}
		to {
			transform: translateX(0);
			opacity: 1;
		}
	}

	.animate-in {
		animation-fill-mode: both;
	}

	.slide-in-from-right {
		animation-name: slide-in-from-right;
	}

	.duration-300 {
		animation-duration: 300ms;
	}
</style>
