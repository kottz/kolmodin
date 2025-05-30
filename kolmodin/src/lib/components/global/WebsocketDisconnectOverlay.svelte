<script lang="ts">
	import { websocketStore, ConnectionStatus } from '$lib/stores/websocket.store.svelte';
	import { Button } from '$lib/components/ui/button';
	import { fade } from 'svelte/transition';
	import { lobbyStore } from '$lib/stores/lobby.store.svelte'; // To get adminId/lobbyId for manual reconnect
	import { notificationStore } from '$lib/stores/notification.store.svelte';

	// Reactive state from the WebSocket store
	const status = $derived(websocketStore.state.status);
	const lastError = $derived(websocketStore.state.lastError);
	const reconnectAttempts = $derived(websocketStore.state.reconnectAttempts);
	const wasManuallyDisconnected = $derived(websocketStore.state.wasManuallyDisconnected); // <<< ADDED

	// Derived state for UI logic
	const showOverlay = $derived(
		(status === ConnectionStatus.DISCONNECTED ||
			status === ConnectionStatus.ERROR ||
			status === ConnectionStatus.RECONNECTING) &&
			!wasManuallyDisconnected // <<< MODIFIED: Do not show if manually disconnected
	);

	const title = $derived(() => {
		switch (status) {
			case ConnectionStatus.RECONNECTING:
				return `Reconnecting (Attempt ${reconnectAttempts})...`;
			case ConnectionStatus.ERROR:
				return 'Connection Error';
			case ConnectionStatus.DISCONNECTED:
				// This message now correctly applies to unexpected disconnections
				return 'Disconnected from Server';
			default:
				// This case should ideally not be hit if showOverlay is true
				return 'Connection Issue';
		}
	});

	const message = $derived(() => {
		if (status === ConnectionStatus.ERROR && lastError) {
			return lastError;
		}
		if (status === ConnectionStatus.DISCONNECTED && lastError) {
			// If overlay is shown for DISCONNECTED, it was unexpected.
			return lastError || 'You have been unexpectedly disconnected.';
		}
		if (status === ConnectionStatus.RECONNECTING) {
			return 'Attempting to restore your session...';
		}
		return '';
	});

	const showManualReconnectButton = $derived(
		status === ConnectionStatus.ERROR &&
			!wasManuallyDisconnected && // Ensures it's not shown if error state resulted from a manual disconnect scenario (though unlikely)
			reconnectAttempts >= websocketStore.MAX_RECONNECT_ATTEMPTS // Assuming MAX_RECONNECT_ATTEMPTS is exported or use literal 5
	);

	function handleManualReconnect() {
		const adminId = lobbyStore.state.adminId;
		const lobbyId = lobbyStore.state.lobbyId;

		if (adminId && lobbyId) {
			// When manually reconnecting, it's a new attempt, so wasManuallyDisconnected should be false.
			// The connect method already handles setting wasManuallyDisconnected = false.
			websocketStore.connect(lobbyId);
		} else {
			notificationStore.add('Cannot reconnect: Session details are missing.', 'destructive');
		}
	}
</script>

{#if showOverlay}
	<div
		class="fixed inset-0 z-[90] flex items-center justify-center bg-black/70 backdrop-blur-sm"
		transition:fade={{ duration: 200 }}
		aria-modal="true"
		role="dialog"
		aria-labelledby="disconnect-title"
	>
		<div class="bg-card text-card-foreground w-full max-w-md rounded-lg p-6 shadow-xl sm:p-8">
			<h2 id="disconnect-title" class="mb-3 text-center text-xl font-semibold sm:text-2xl">
				{title()}
			</h2>

			{#if status === ConnectionStatus.RECONNECTING}
				<div class="mb-4 flex justify-center">
					<div
						class="border-primary h-10 w-10 animate-spin rounded-full border-4 border-t-transparent"
					></div>
				</div>
			{/if}

			{#if message()}
				<p class="text-muted-foreground mb-6 text-center text-sm sm:text-base">{message()}</p>
			{/if}

			{#if showManualReconnectButton}
				<div class="mt-4 flex flex-col items-center space-y-3">
					<p class="text-muted-foreground text-center text-xs">Automatic reconnection failed.</p>
					<Button onclick={handleManualReconnect} class="w-full max-w-xs sm:w-auto">
						Try to Reconnect
					</Button>
				</div>
			{/if}
		</div>
	</div>
{/if}
