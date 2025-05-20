<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { info } from '$lib/utils/logger';

	function handleCreateLobbyClick(): void {
		info('HomeScreen: "Create New Lobby" clicked.');
		uiStore.navigateToSelectGame();
	}
</script>

<div class="flex min-h-[calc(100vh-4rem)] flex-col items-center justify-center p-4 sm:p-6 md:p-8">
	<div class="w-full max-w-md space-y-8 text-center">
		<div>
			<!-- You can add a logo or app icon here if you have one -->
			<!-- <img class="mx-auto h-12 w-auto" src="/path/to/your/logo.svg" alt="App Logo" /> -->
			<h1 class="text-foreground mt-6 text-3xl font-bold tracking-tight sm:text-4xl">
				Game Admin Panel
			</h1>
			<p class="text-md text-muted-foreground mt-3 sm:text-lg">
				Welcome! Create a new game lobby to get started.
			</p>
		</div>

		<div class="mt-8">
			<Button
				onclick={handleCreateLobbyClick}
				size="lg"
				class="w-full max-w-xs transform transition-transform duration-150 ease-in-out hover:scale-105 active:scale-95"
			>
				Create New Lobby
			</Button>
		</div>

		<footer class="text-muted-foreground mt-12 pt-8 text-center text-sm">
			<!-- Optional footer content -->
			<!-- <p>© {new Date().getFullYear()} Your Company Name. All rights reserved.</p> -->
			<!-- <p>
				<a href="/terms" class="hover:text-primary">Terms of Service</a> |
				<a href="/privacy" class="hover:text-primary">Privacy Policy</a>
			</p> -->
		</footer>
	</div>
</div>

<style lang="postcss">
	/* Add any specific styles for HomeScreen if needed, beyond Tailwind classes */
	/* For example, if you want a background image or gradient specific to this screen */
</style>
