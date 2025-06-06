<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { broadcastService } from '$lib/services/broadcast.service';
	import { Monitor, Eye, EyeOff, ExternalLink } from 'lucide-svelte';
	import { info } from '$lib/utils/logger';

	interface Props {
		class?: string;
	}

	let { class: className = '' }: Props = $props();

	const canOpenStream = $derived(uiStore.canOpenStreamWindow);
	const streamWindowState = $derived(uiStore.streamWindow);
	let isStreamVisible = $state(true); // Track visibility state

	function handleOpenStreamWindow(): void {
		info('StreamControls: Opening stream window');
		uiStore.openStreamWindow();
	}

	function handleCloseStreamWindow(): void {
		info('StreamControls: Closing stream window');
		uiStore.closeStreamWindow();
	}

	function handleToggleVisibility(): void {
		if (isStreamVisible) {
			info('StreamControls: Hiding stream');
			broadcastService.broadcastStreamControl('hide');
		} else {
			info('StreamControls: Showing stream');
			broadcastService.broadcastStreamControl('show');
		}
		isStreamVisible = !isStreamVisible;
	}

	function handleFocusStream(): void {
		if (streamWindowState.window && !streamWindowState.window.closed) {
			streamWindowState.window.focus();
		}
	}
</script>

<!-- Simplified Stream Controls Card -->
<Card class={className}>
	<CardHeader>
		<CardTitle class="flex items-center gap-2">
			<Monitor class="h-5 w-5" />
			Stream Window
		</CardTitle>
	</CardHeader>
	<CardContent>
		{#if !canOpenStream}
			<div class="text-muted-foreground py-4 text-center">
				<Monitor class="mx-auto mb-2 h-8 w-8 opacity-50" />
				<p class="text-sm">Stream controls available when game is active</p>
			</div>
		{:else if streamWindowState.isOpen}
			<!-- Window is open: Focus + Close + Visibility Toggle -->
			<div class="space-y-2">
				<div class="flex gap-2">
					<Button onclick={handleFocusStream} variant="default" size="sm" class="flex-1">
						<Monitor class="mr-2 h-4 w-4" />
						Focus
					</Button>
					<Button onclick={handleCloseStreamWindow} variant="outline" size="sm" class="flex-1">
						<ExternalLink class="mr-2 h-4 w-4" />
						Close
					</Button>
				</div>
				<Button
					onclick={handleToggleVisibility}
					variant="outline"
					size="sm"
					class="w-full"
					title={isStreamVisible ? 'Hide Stream' : 'Show Stream'}
				>
					{#if isStreamVisible}
						<Eye class="mr-2 h-4 w-4" />
						Hide Stream
					{:else}
						<EyeOff class="mr-2 h-4 w-4" />
						Show Stream
					{/if}
				</Button>
			</div>
		{:else}
			<!-- Window is closed: Just Open button -->
			<Button onclick={handleOpenStreamWindow} variant="default" size="sm" class="w-full">
				<Monitor class="mr-2 h-4 w-4" />
				Open Stream Window
			</Button>
		{/if}
	</CardContent>
</Card>
