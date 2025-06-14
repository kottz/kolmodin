<script lang="ts">
	import { Card, CardContent } from '$lib/components/ui/card';
	import { info, warn } from '$lib/utils/logger';

	interface Props {
		onVideoEnded?: () => void;
	}

	let { onVideoEnded }: Props = $props();

	let playerId = 'youtube-player';
	let player: YT.Player | null = null;

	function onPlayerReady(event: YT.PlayerEvent) {
		info('Player ready');
		player = event.target;
		// Expose player globally for AdminView to use
		(window as unknown).clipQueuePlayer = player;
	}

	function onPlayerStateChange(event: YT.OnStateChangeEvent) {
		info('YouTube Player state changed:', event.data);

		// Handle video ended - call callback prop
		if (event.data === YT.PlayerState.ENDED) {
			info('Video ended');
			if (onVideoEnded) {
				onVideoEnded();
			}
		}
	}

	function onPlayerError(event: YT.OnErrorEvent) {
		warn('YouTube player error:', event);
	}

	function createPlayer() {
		info('Creating YouTube player');
		player = new YT.Player(playerId, {
			height: '360',
			width: '640',
			playerVars: {
				controls: 1,
				playsinline: 1,
				enablejsapi: 1
			},
			events: {
				onReady: onPlayerReady,
				onStateChange: onPlayerStateChange,
				onError: onPlayerError
			}
		});
	}

	// Initialize immediately - no lifecycle functions
	info('Loading YouTube API');
	if ((window as unknown).YT && (window as unknown).YT.Player) {
		// API already loaded
		setTimeout(createPlayer, 100);
	} else {
		// Load API first
		const tag = document.createElement('script');
		tag.src = 'https://www.youtube.com/iframe_api';
		const firstScriptTag = document.getElementsByTagName('script')[0];
		firstScriptTag.parentNode?.insertBefore(tag, firstScriptTag);

		// Set global callback
		(window as unknown).onYouTubeIframeAPIReady = createPlayer;
	}
</script>

<Card>
	<CardContent class="p-1">
		<div class="bg-muted aspect-video w-full">
			<div id={playerId}></div>
		</div>
	</CardContent>
</Card>

<style>
	#youtube-player {
		width: 100%;
		height: 100%;
	}
</style>
