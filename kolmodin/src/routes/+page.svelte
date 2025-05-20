<script lang="ts">
	import { uiStore, type AppScreen } from '$lib/stores/ui.store.svelte';
	import HomeScreen from '$lib/components/screens/HomeScreen.svelte';
	import SelectGameScreen from '$lib/components/screens/SelectGameScreen.svelte';
	import GameScreen from '$lib/components/screens/GameScreen.svelte';
	import { log } from '$lib/utils/logger';

	let currentScreen = $derived(uiStore.state.currentScreen);
	let ScreenComponent = $derived(getScreenComponent(currentScreen));

	function getScreenComponent(screen: AppScreen) {
		switch (screen) {
			case 'home':
				return HomeScreen;
			case 'selectGame':
				return SelectGameScreen;
			case 'gameActive':
				return GameScreen;
			default:
				// Log the error and ensure a fallback.
				// Note: Svelte's $derived might re-evaluate this function more than expected
				// if currentScreen somehow becomes an invalid value temporarily.
				// Consider how often this log might fire if currentScreen is unstable.
				log('Error: Unknown screen in uiStore:', screen, '- Defaulting to HomeScreen.');
				return HomeScreen;
		}
	}
</script>

{#if ScreenComponent}
	<!-- Correct Svelte 5 dynamic component rendering -->
	<ScreenComponent />
{:else}
	<div class="flex h-screen items-center justify-center">
		<p class="text-destructive-foreground text-xl">
			Error: Could not determine the current screen component.
		</p>
	</div>
{/if}
