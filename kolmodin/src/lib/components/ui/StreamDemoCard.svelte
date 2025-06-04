<script lang="ts">
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { Button } from '$lib/components/ui/button';
	import Badge from '$lib/components/ui/badge.svelte';
	import { broadcastService } from '$lib/services/broadcast.service';
	import { gameEventRouter } from '$lib/services/game.event.router';
	import { uiStore } from '$lib/stores/ui.store.svelte';
	import { Monitor, Zap, Users, Timer } from 'lucide-svelte';
	import { info } from '$lib/utils/logger';

	interface Props {
		class?: string;
	}

	let { class: className = '' }: Props = $props();

	const currentGame = $derived(gameEventRouter.getActiveGame());
	const streamWindowOpen = $derived(uiStore.streamWindow.isOpen);

	function handleTestBroadcast() {
		if (!currentGame) {
			info('StreamDemoCard: No active game to test broadcast');
			return;
		}

		// Simulate some test stream events
		broadcastService.broadcastStreamEvent(currentGame, {
			type: 'DEMO_EVENT',
			data: {
				message: '🧪 Test broadcast successful!',
				timestamp: new Date().toLocaleTimeString()
			},
			duration: 3000,
			timestamp: Date.now()
		});

		info('StreamDemoCard: Test broadcast sent');
	}

	function handleForceStateUpdate() {
		gameEventRouter.broadcastCurrentGameState();
		info('StreamDemoCard: Forced state update broadcast');
	}

	function handleQuickStreamSetup() {
		// Quick setup: open window and show stream
		uiStore.openStreamWindow();
		setTimeout(() => {
			broadcastService.broadcastStreamControl('show');
		}, 1000);
		info('StreamDemoCard: Quick stream setup initiated');
	}

	function getBroadcastStatus(): string {
		if (!broadcastService.isInitialized()) return 'Not initialized';
		if (broadcastService.getIsStreamWindow()) return 'Stream window';
		return 'Admin window (broadcasting)';
	}
</script>

<Card class={className}>
	<CardHeader>
		<CardTitle class="flex items-center gap-2">
			<Monitor class="h-5 w-5" />
			Stream System Status
		</CardTitle>
	</CardHeader>
	<CardContent class="space-y-4">
		<!-- Status Overview -->
		<div class="grid grid-cols-2 gap-3">
			<div class="space-y-2">
				<div class="flex items-center gap-2 text-sm">
					<Users class="h-4 w-4" />
					<span>Active Game:</span>
				</div>
				<Badge variant={currentGame ? 'default' : 'secondary'} class="w-full justify-center">
					{currentGame || 'None'}
				</Badge>
			</div>

			<div class="space-y-2">
				<div class="flex items-center gap-2 text-sm">
					<Monitor class="h-4 w-4" />
					<span>Stream Window:</span>
				</div>
				<Badge variant={streamWindowOpen ? 'default' : 'outline'} class="w-full justify-center">
					{streamWindowOpen ? 'Open' : 'Closed'}
				</Badge>
			</div>

			<div class="space-y-2">
				<div class="flex items-center gap-2 text-sm">
					<Zap class="h-4 w-4" />
					<span>Broadcast:</span>
				</div>
				<Badge variant="outline" class="w-full justify-center text-xs">
					{getBroadcastStatus()}
				</Badge>
			</div>

			<div class="space-y-2">
				<div class="flex items-center gap-2 text-sm">
					<Timer class="h-4 w-4" />
					<span>Status:</span>
				</div>
				<Badge
					variant={currentGame && streamWindowOpen ? 'default' : 'secondary'}
					class="w-full justify-center"
				>
					{currentGame && streamWindowOpen ? 'Ready' : 'Setup Needed'}
				</Badge>
			</div>
		</div>

		{#if currentGame}
			<!-- Test Controls -->
			<div class="space-y-3">
				<h4 class="text-sm font-medium">Test Controls</h4>
				<div class="grid grid-cols-1 gap-2">
					<Button
						onclick={handleQuickStreamSetup}
						variant="default"
						size="sm"
						disabled={streamWindowOpen}
						class="w-full"
					>
						<Monitor class="mr-2 h-4 w-4" />
						Quick Setup (Open + Show)
					</Button>

					<div class="grid grid-cols-2 gap-2">
						<Button
							onclick={handleTestBroadcast}
							variant="outline"
							size="sm"
							disabled={!streamWindowOpen}
						>
							<Zap class="mr-2 h-4 w-4" />
							Test Event
						</Button>

						<Button
							onclick={handleForceStateUpdate}
							variant="outline"
							size="sm"
							disabled={!streamWindowOpen}
						>
							<Users class="mr-2 h-4 w-4" />
							Force Update
						</Button>
					</div>
				</div>
			</div>

			<!-- Usage Instructions -->
			<div class="bg-muted/50 rounded-md p-3">
				<h5 class="mb-2 text-xs font-semibold">How to Test:</h5>
				<ol class="text-muted-foreground space-y-1 text-xs">
					<li>1. Click "Quick Setup" to open stream window</li>
					<li>2. Use "Test Event" to send demo messages</li>
					<li>3. Watch stream window for real-time updates</li>
					<li>4. Play the game normally - events auto-broadcast!</li>
				</ol>
			</div>
		{:else}
			<!-- No Game Active -->
			<div class="bg-muted/50 rounded-md p-4 text-center">
				<Monitor class="mx-auto mb-2 h-8 w-8 opacity-50" />
				<p class="text-muted-foreground text-sm">Start a game to enable streaming features</p>
			</div>
		{/if}
	</CardContent>
</Card>
