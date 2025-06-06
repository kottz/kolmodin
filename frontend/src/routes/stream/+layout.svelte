<script lang="ts">
	import '../../app.css'; // Import global styles and Tailwind
	import { ModeWatcher } from 'mode-watcher';

	// Note: We deliberately don't import NotificationList or WebsocketDisconnectOverlay
	// as the stream window shouldn't show these admin-specific components

	let { children } = $props();
</script>

<svelte:head>
	<title>Game Stream View</title>
	<meta name="robots" content="noindex, nofollow" />
	<meta name="viewport" content="width=device-width, initial-scale=1" />
</svelte:head>

<!-- Force dark mode for stream window for better stream appearance -->
<ModeWatcher defaultMode={'dark'} disableTransitions />

<main class="bg-background text-foreground">
	{@render children()}
</main>
