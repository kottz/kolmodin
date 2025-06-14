<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { X, Play } from 'lucide-svelte';
	import type { ClipInfo } from '../types';
	import { formatDuration, getTimeAgo } from '../types';
	import { clipQueueStore } from '../store.svelte';

	interface Props {
		clip: ClipInfo;
		isCurrentlyPlaying?: boolean;
		onPlayClip?: (videoId: string) => void;
	}

	let { clip, isCurrentlyPlaying = false, onPlayClip }: Props = $props();

	function handlePlayClip() {
		if (onPlayClip) {
			onPlayClip(clip.videoId);
		}
	}

	function handleRemoveClip() {
		clipQueueStore.actions.removeClipFromQueue(clip.videoId);
	}
</script>

<div
	class="group bg-card hover:bg-muted/50 flex items-center gap-3 rounded-lg border p-3 transition-all {isCurrentlyPlaying
		? 'ring-primary bg-muted/30 ring-2'
		: ''}"
	role="button"
	tabindex="0"
	onclick={handlePlayClip}
	onkeydown={(e) => e.key === 'Enter' && handlePlayClip()}
>
	<!-- Thumbnail -->
	<div class="relative shrink-0">
		<img
			src={clip.thumbnailUrl}
			alt="{clip.title} thumbnail"
			class="bg-muted h-12 w-16 rounded object-cover"
			loading="lazy"
		/>
		{#if isCurrentlyPlaying}
			<div class="bg-primary/20 absolute inset-0 flex items-center justify-center rounded">
				<Play class="text-primary-foreground h-4 w-4" fill="currentColor" />
			</div>
		{/if}
	</div>

	<!-- Content -->
	<div class="min-w-0 flex-1">
		<div class="flex items-start justify-between gap-2">
			<div class="min-w-0 flex-1">
				<h4
					class="text-foreground line-clamp-1 text-sm leading-tight font-medium"
					title={clip.title}
				>
					{clip.title}
				</h4>
				<p class="text-muted-foreground line-clamp-1 text-xs" title={clip.channelTitle}>
					{clip.channelTitle}
				</p>
			</div>

			<!-- Duration -->
			<span class="text-muted-foreground shrink-0 font-mono text-xs">
				{formatDuration(clip.durationIso8601)}
			</span>
		</div>

		<!-- Submission info -->
		<div class="text-muted-foreground mt-1 flex items-center justify-between text-xs">
			<span>by {clip.submittedByUsername}</span>
			<span>{getTimeAgo(clip.submittedAtTimestamp)}</span>
		</div>
	</div>

	<!-- Remove button -->
	<Button
		variant="ghost"
		size="sm"
		class="h-8 w-8 p-0 opacity-0 transition-opacity group-hover:opacity-100"
		onclick={(e) => {
			e.stopPropagation();
			handleRemoveClip();
		}}
		title="Remove clip"
	>
		<X class="text-destructive h-4 w-4" />
	</Button>
</div>

<style>
	.line-clamp-1 {
		display: -webkit-box;
		-webkit-line-clamp: 1;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
</style>
