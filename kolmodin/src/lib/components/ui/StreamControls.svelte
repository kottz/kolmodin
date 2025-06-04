<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { broadcastService } from '$lib/services/broadcast.service';
	import { Monitor, Eye, EyeOff, RotateCcw, ExternalLink } from 'lucide-svelte';
	import { info } from '$lib/utils/logger';

	interface Props {
		compact?: boolean; // Whether to show in compact mode
		class?: string;
	}

	let { compact = false, class: className = '' }: Props = $props();

	const canOpenStream = $derived(uiStore.canOpenStreamWindow);
	const streamWindowState = $derived(uiStore.streamWindow);

	function handleOpenStreamWindow(): void {
		info('StreamControls: Opening stream window');
		uiStore.openStreamWindow();
	}

	function handleCloseStreamWindow(): void {
		info('StreamControls: Closing stream window');
		uiStore.closeStreamWindow();
	}

	function handleToggleStreamWindow(): void {
		info('StreamControls: Toggling stream window');
		uiStore.toggleStreamWindow();
	}

	function handleShowStream(): void {
		info('StreamControls: Showing stream');
		broadcastService.broadcastStreamControl('show');
	}

	function handleHideStream(): void {
		info('StreamControls: Hiding stream');
		broadcastService.broadcastStreamControl('hide');
	}

	function handleClearStream(): void {
		info('StreamControls: Clearing stream');
		broadcastService.broadcastStreamControl('clear');
	}

	function handleFocusStream(): void {
		if (streamWindowState.window && !streamWindowState.window.closed) {
			streamWindowState.window.focus();
		}
	}
</script>

{#if compact}
	<!-- Compact mode - just buttons -->
	<div class="flex flex-wrap gap-2 {className}">
		{#if canOpenStream}
			{#if streamWindowState.isOpen}
				<Button onclick={handleFocusStream} variant="outline" size="sm" title="Focus Stream Window">
					<Monitor class="h-4 w-4" />
				</Button>
				<Button
					onclick={handleCloseStreamWindow}
					variant="outline"
					size="sm"
					title="Close Stream Window"
				>
					<ExternalLink class="h-4 w-4" />
				</Button>
			{:else}
				<Button
					onclick={handleOpenStreamWindow}
					variant="outline"
					size="sm"
					title="Open Stream Window"
				>
					<Monitor class="h-4 w-4" />
				</Button>
			{/if}

			<Button onclick={handleShowStream} variant="outline" size="sm" title="Show Stream">
				<Eye class="h-4 w-4" />
			</Button>
			<Button onclick={handleHideStream} variant="outline" size="sm" title="Hide Stream">
				<EyeOff class="h-4 w-4" />
			</Button>
			<Button onclick={handleClearStream} variant="outline" size="sm" title="Clear Stream">
				<RotateCcw class="h-4 w-4" />
			</Button>
		{:else}
			<span class="text-muted-foreground text-sm italic">No active game</span>
		{/if}
	</div>
{:else}
	<!-- Full card mode -->
	<Card class={className}>
		<CardHeader>
			<CardTitle class="flex items-center gap-2">
				<Monitor class="h-5 w-5" />
				Stream Controls
			</CardTitle>
		</CardHeader>
		<CardContent class="space-y-4">
			{#if !canOpenStream}
				<div class="text-muted-foreground py-4 text-center">
					<Monitor class="mx-auto mb-2 h-8 w-8 opacity-50" />
					<p class="text-sm">Stream controls available when game is active</p>
				</div>
			{:else}
				<!-- Window Management -->
				<div class="space-y-3">
					<h4 class="text-sm font-medium">Window Management</h4>
					<div class="flex flex-wrap gap-2">
						{#if streamWindowState.isOpen}
							<Button
								onclick={handleFocusStream}
								variant="outline"
								size="sm"
								class="min-w-[120px] flex-1"
							>
								<Monitor class="mr-2 h-4 w-4" />
								Focus Window
							</Button>
							<Button
								onclick={handleCloseStreamWindow}
								variant="outline"
								size="sm"
								class="min-w-[120px] flex-1"
							>
								<ExternalLink class="mr-2 h-4 w-4" />
								Close Window
							</Button>
						{:else}
							<Button onclick={handleOpenStreamWindow} variant="default" size="sm" class="w-full">
								<Monitor class="mr-2 h-4 w-4" />
								Open Stream Window
							</Button>
						{/if}
					</div>

					{#if streamWindowState.isOpen}
						<div class="bg-muted/50 rounded-md p-2">
							<p class="text-muted-foreground text-sm">
								Stream window is open. Use the controls below to manage what's displayed.
							</p>
						</div>
					{/if}
				</div>

				<!-- Stream Display Controls -->
				<div class="space-y-3">
					<h4 class="text-sm font-medium">Display Controls</h4>
					<div class="grid grid-cols-3 gap-2">
						<Button onclick={handleShowStream} variant="outline" size="sm">
							<Eye class="mr-2 h-4 w-4" />
							Show
						</Button>
						<Button onclick={handleHideStream} variant="outline" size="sm">
							<EyeOff class="mr-2 h-4 w-4" />
							Hide
						</Button>
						<Button onclick={handleClearStream} variant="outline" size="sm">
							<RotateCcw class="mr-2 h-4 w-4" />
							Clear
						</Button>
					</div>
					<div class="text-muted-foreground space-y-1 text-xs">
						<p><strong>Show:</strong> Make stream visible</p>
						<p><strong>Hide:</strong> Hide stream content</p>
						<p><strong>Clear:</strong> Clear all events and reset display</p>
					</div>
				</div>

				<!-- Quick Actions -->
				<div class="space-y-3">
					<h4 class="text-sm font-medium">Quick Actions</h4>
					<div class="flex gap-2">
						<Button
							onclick={() => {
								handleOpenStreamWindow();
								setTimeout(handleShowStream, 500);
							}}
							variant="default"
							size="sm"
							class="flex-1"
							disabled={streamWindowState.isOpen}
						>
							Open & Show
						</Button>
						<Button
							onclick={() => {
								handleClearStream();
								handleHideStream();
							}}
							variant="secondary"
							size="sm"
							class="flex-1"
						>
							Clear & Hide
						</Button>
					</div>
				</div>
			{/if}
		</CardContent>
	</Card>
{/if}
