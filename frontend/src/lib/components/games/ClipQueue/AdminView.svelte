<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Checkbox from '$lib/components/ui/checkbox.svelte';
	import Label from '$lib/components/ui/label.svelte';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import {
		Play,
		SkipForward,
		RotateCcw,
		Lock,
		Unlock,
		Settings,
		Users,
		Video
	} from 'lucide-svelte';

	import { clipQueueStore } from './store.svelte';
	import { websocketStore } from '$lib/stores/websocket.store.svelte';
	import ClipQueueItem from './components/ClipQueueItem.svelte';
	import ClipQueueYouTubePlayer from './components/ClipQueueYouTubePlayer.svelte';
	import type { ClipQueueSettings } from './types';

	// Get reactive state from store
	const store = clipQueueStore;
	const state = $derived(store.state);
	const computed = $derived(store.computed);

	// Client-side only settings
	let autoplayNext = $state(true);

	// Use server settings directly - no local copy
	const currentSettings = $derived(
		state?.settings || {
			submissionsOpen: true,
			allowDuplicates: false,
			maxClipDurationSeconds: 600
		}
	);

	// Client-side playlist management
	let currentPlayingIndex = $state(-1); // -1 means not playing

	// Computed values for client-side playback
	const playback = $derived({
		currentClip: currentPlayingIndex >= 0 ? state?.clipQueue?.[currentPlayingIndex] : null,
		hasNext: currentPlayingIndex < (state?.clipQueue?.length || 0) - 1,
		hasPrevious: currentPlayingIndex > 0,
		isPlaying: currentPlayingIndex >= 0
	});

	// Auto-advance handler - called from YouTube player
	function handleVideoEnded() {
		if (autoplayNext && playback.hasNext) {
			console.log('Auto-advancing to next clip');
			setTimeout(() => {
				currentPlayingIndex++;
				const nextClip = state?.clipQueue?.[currentPlayingIndex];
				if (nextClip) {
					loadAndPlayVideo(nextClip.videoId);
				}
			}, 1000);
		} else {
			console.log('Video ended, stopping playback');
			currentPlayingIndex = -1;
			stopPlayback();
		}
	}

	// Direct YouTube API calls - no stores
	function loadAndPlayVideo(videoId: string) {
		console.log('Loading and playing video:', videoId);
		const player = (window as unknown).clipQueuePlayer as YT.Player;
		if (player) {
			player.loadVideoById(videoId);
		}
	}

	function stopPlayback() {
		console.log('Stopping playback');
		const player = (window as unknown).clipQueuePlayer as YT.Player;
		if (player) {
			player.stopVideo();
		}
	}

	function handlePlayNextClip() {
		console.log('AdminView: Play Next Clip clicked');
		if (playback.hasNext) {
			currentPlayingIndex++;
			const nextClip = state?.clipQueue?.[currentPlayingIndex];
			if (nextClip) {
				loadAndPlayVideo(nextClip.videoId);
			}
		} else if ((state?.clipQueue?.length || 0) > 0) {
			// Start from beginning if no current clip
			currentPlayingIndex = 0;
			const firstClip = state?.clipQueue?.[0];
			if (firstClip) {
				loadAndPlayVideo(firstClip.videoId);
			}
		}
	}

	function handlePlaySpecificClip(videoId: string) {
		// Find the index of the clip in the queue
		const index = state?.clipQueue?.findIndex((clip) => clip.videoId === videoId) ?? -1;
		if (index >= 0) {
			currentPlayingIndex = index;
			loadAndPlayVideo(videoId);
		}
	}

	function handleResetQueue() {
		if (
			confirm(
				'Are you sure you want to reset the queue? This will clear all clips and cannot be undone.'
			)
		) {
			store.actions.resetQueue();
		}
	}

	function updateSetting(key: keyof ClipQueueSettings, value: boolean | number) {
		// Only send settings if connected
		if (websocketStore.state.status === 'CONNECTED') {
			const newSettings = { ...currentSettings, [key]: value };
			store.actions.updateSettings(newSettings);
		}
	}
</script>

<div class="flex h-full gap-4 p-4">
	<!-- Main content - YouTube Player -->
	<div class="flex flex-1 flex-col gap-4">
		<!-- YouTube Player Area -->
		<Card class="flex-1">
			<CardContent class="p-0">
				<div class="bg-muted/30 relative aspect-video w-full overflow-hidden rounded-lg">
					<ClipQueueYouTubePlayer onVideoEnded={handleVideoEnded} />
				</div>
			</CardContent>
		</Card>

		<!-- Playback Controls -->
		<Card>
			<CardContent class="p-4">
				<div class="flex items-center justify-center gap-3">
					<Button
						onclick={handlePlayNextClip}
						disabled={!playback.hasNext && (state?.clipQueue?.length || 0) === 0}
						size="lg"
						class="gap-2"
					>
						{#if playback.isPlaying}
							<SkipForward class="h-4 w-4" />
							Skip to Next
						{:else}
							<Play class="h-4 w-4" />
							Start Playing
						{/if}
					</Button>

					<Button
						onclick={handleResetQueue}
						variant="outline"
						size="lg"
						class="gap-2"
						disabled={(state?.clipQueue?.length || 0) === 0}
					>
						<RotateCcw class="h-4 w-4" />
						Reset Queue
					</Button>
				</div>

				{#if playback.currentClip}
					<div class="mt-4 text-center">
						<p class="text-muted-foreground text-sm">
							Now Playing ({currentPlayingIndex + 1} of {state?.clipQueue?.length || 0}):
						</p>
						<p class="font-medium">{playback.currentClip.title}</p>
						<p class="text-muted-foreground text-sm">
							by {playback.currentClip.channelTitle}
						</p>
					</div>
				{/if}
			</CardContent>
		</Card>
	</div>

	<!-- Sidebar -->
	<div class="flex w-80 flex-col gap-4">
		<!-- Settings -->
		<Card>
			<CardHeader class="pb-3">
				<CardTitle class="flex items-center gap-2 text-base">
					<Settings class="h-4 w-4" />
					Settings
				</CardTitle>
			</CardHeader>
			<CardContent class="space-y-4">
				<!-- Submissions Toggle -->
				<div class="flex items-center justify-between">
					<Label for="submissions-open" class="flex items-center gap-2">
						{#if currentSettings.submissionsOpen}
							<Unlock class="h-4 w-4 text-green-500" />
						{:else}
							<Lock class="h-4 w-4 text-red-500" />
						{/if}
						Submissions
					</Label>
					<Checkbox
						id="submissions-open"
						checked={currentSettings.submissionsOpen}
						onCheckedChange={(checked) => updateSetting('submissionsOpen', checked)}
					/>
				</div>

				<!-- Allow Duplicates -->
				<div class="flex items-center justify-between">
					<Label for="allow-duplicates">Allow Duplicates</Label>
					<Checkbox
						id="allow-duplicates"
						checked={currentSettings.allowDuplicates}
						onCheckedChange={(checked) => updateSetting('allowDuplicates', checked)}
					/>
				</div>

				<!-- Autoplay Next -->
				<div class="flex items-center justify-between">
					<Label for="autoplay-next">Autoplay Next</Label>
					<Checkbox id="autoplay-next" bind:checked={autoplayNext} />
				</div>

				<!-- Max Duration -->
				<div class="space-y-2">
					<Label for="max-duration">Max Duration (seconds)</Label>
					<Input
						id="max-duration"
						type="number"
						min="30"
						max="3600"
						value={currentSettings.maxClipDurationSeconds}
						onchange={(e) => updateSetting('maxClipDurationSeconds', parseInt(e.target.value))}
						class="w-full"
					/>
				</div>
			</CardContent>
		</Card>

		<!-- Queue -->
		<Card class="flex flex-1 flex-col">
			<CardHeader class="pb-3">
				<CardTitle class="flex items-center justify-between text-base">
					<span class="flex items-center gap-2">
						<Users class="h-4 w-4" />
						Queue
					</span>
					<span class="text-muted-foreground text-sm font-normal">
						{computed?.queueCount || 0} clips
					</span>
				</CardTitle>
			</CardHeader>
			<CardContent class="flex-1 p-0">
				{#if (state?.clipQueue?.length || 0) === 0}
					<div class="flex h-32 items-center justify-center p-4 text-center">
						<div>
							<Video class="text-muted-foreground mx-auto mb-2 h-8 w-8" />
							<p class="text-muted-foreground text-sm">No clips in queue</p>
							<p class="text-muted-foreground mt-1 text-xs">
								Viewers can submit clips with <code class="bg-muted rounded px-1">!clip [url]</code>
							</p>
						</div>
					</div>
				{:else}
					<ScrollArea class="h-full">
						<div class="space-y-2 p-4">
							{#each state?.clipQueue || [] as clip, index (clip.videoId)}
								<ClipQueueItem
									{clip}
									isCurrentlyPlaying={currentPlayingIndex === index}
									onPlayClip={handlePlaySpecificClip}
								/>
							{/each}
						</div>
					</ScrollArea>
				{/if}
			</CardContent>
		</Card>
	</div>
</div>

<style>
	code {
		font-family:
			ui-monospace, SFMono-Regular, 'SF Mono', Consolas, 'Liberation Mono', Menlo, monospace;
	}
</style>
