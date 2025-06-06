<script lang="ts">
	import '../app.css'; // Import global styles and Tailwind
	import { ModeWatcher } from 'mode-watcher'; // If you use mode-watcher
	import NotificationList from '$lib/components/global/NotificationList.svelte';
	import WebsocketDisconnectOverlay from '$lib/components/global/WebsocketDisconnectOverlay.svelte';
	import { PUBLIC_APP_TITLE } from '$env/static/public'; // For browser tab title

	// This 'children' prop is implicitly provided by SvelteKit for layouts
	let { children } = $props();
</script>

<svelte:head>
	<title>{PUBLIC_APP_TITLE || 'Game Admin Panel'}</title>
	<!-- Add any other global head elements, like favicons or meta tags -->
</svelte:head>

<!-- If using mode-watcher for dark/light mode -->
<ModeWatcher defaultMode="dark" />

<NotificationList />
<WebsocketDisconnectOverlay />

<main class="bg-background text-foreground min-h-screen">
	{@render children()}
</main>
