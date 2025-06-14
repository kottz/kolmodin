import { websocketStore } from '$lib/stores/websocket.store.svelte';
import { registerGameStore } from '$lib/services/game.event.router';
import type { StreamEvent, BasePublicGameState } from '$lib/types/stream.types';
import type { ClipQueueFullState, ClipQueueSettings, ClipQueueEventData, ClipInfo } from './types';
import { info, warn, error } from '$lib/utils/logger';

const GAME_TYPE_ID = 'ClipQueue';

// Transform server clip data (snake_case) to frontend format (camelCase)
function transformClipFromServer(serverClip: any): ClipInfo {
	return {
		videoId: serverClip.video_id,
		title: serverClip.title,
		channelTitle: serverClip.channel_title,
		durationIso8601: serverClip.duration_iso8601,
		thumbnailUrl: serverClip.thumbnail_url,
		submittedByUsername: serverClip.submitted_by_username,
		submittedAtTimestamp: serverClip.submitted_at_timestamp
	};
}

// Interface for public state (what's safe to broadcast to stream window)
interface ClipQueuePublicState extends BasePublicGameState {
	phase: { type: string; currentClipVideoId?: string };
	currentClip: ClipInfo | null;
	queueCount: number;
	settings: ClipQueueSettings;
}

function createInitialClipQueueState(): ClipQueueFullState {
	return {
		phase: { name: 'Idle' },
		clipQueue: [],
		playedClipIds: [],
		removedByAdminClipIds: [],
		settings: {
			submissionsOpen: true,
			allowDuplicates: false,
			maxClipDurationSeconds: 600
		}
	};
}

function createClipQueueStore() {
	const gameState = $state<ClipQueueFullState>(createInitialClipQueueState());
	const streamEvents = $state<StreamEvent[]>([]);

	// Actions for sending commands to backend
	const actions = {
		removeClipFromQueue(videoId: string) {
			info('ClipQueue: Removing clip from queue', videoId);
			websocketStore.send({
				messageType: 'GameSpecificCommand',
				payload: {
					game_type_id: 'ClipQueue',
					command_data: {
						command: 'RemoveClipFromQueue',
						video_id: videoId
					}
				}
			});
		},

		updateSettings(newSettings: ClipQueueSettings) {
			info('ClipQueue: Updating settings', newSettings);
			// Convert camelCase frontend settings to snake_case for server
			const serverSettings = {
				submissions_open: newSettings.submissionsOpen,
				allow_duplicates: newSettings.allowDuplicates,
				max_clip_duration_seconds: newSettings.maxClipDurationSeconds
			};
			websocketStore.send({
				messageType: 'GameSpecificCommand',
				payload: {
					game_type_id: 'ClipQueue',
					command_data: {
						command: 'UpdateSettings',
						new_settings: serverSettings
					}
				}
			});
		},

		resetQueue() {
			info('ClipQueue: Resetting queue');
			websocketStore.send({
				messageType: 'GameSpecificCommand',
				payload: {
					game_type_id: 'ClipQueue',
					command_data: {
						command: 'ResetQueue'
					}
				}
			});
		}
	};

	// Process incoming events from the backend
	function processEvent(eventData: any): void {
		if (!eventData || typeof eventData !== 'object') {
			warn('ClipQueue: Received invalid event data', eventData);
			return;
		}

		const data = eventData as ClipQueueEventData;

		// Handle events based on event_type
		if ('event_type' in data) {
			switch (data.event_type) {
				case 'FullStateUpdate':
					if (
						'data' in data &&
						data.data &&
						typeof data.data === 'object' &&
						'state' in data.data
					) {
						const serverState = data.data.state as any;
						info('ClipQueue: Full state update received', serverState);
						info(
							'ClipQueue: Current clipQueue length before update:',
							gameState.clipQueue?.length || 0
						);

						// Transform server format (snake_case) to frontend format (camelCase)
						const newState: ClipQueueFullState = {
							phase: serverState.phase,
							clipQueue: (serverState.clip_queue || []).map(transformClipFromServer),
							playedClipIds: serverState.played_clip_ids || [],
							removedByAdminClipIds: serverState.removed_by_admin_clip_ids || [],
							settings: {
								submissionsOpen: serverState.settings?.submissions_open ?? true,
								allowDuplicates: serverState.settings?.allow_duplicates ?? false,
								maxClipDurationSeconds: serverState.settings?.max_clip_duration_seconds ?? 600
							}
						};

						// Update state using Object.assign to preserve reactivity
						Object.assign(gameState, newState);

						info('ClipQueue: New clipQueue length after update:', gameState.clipQueue?.length || 0);
					}
					break;

				case 'ClipAdded':
					if ('data' in data && data.data && typeof data.data === 'object' && 'clip' in data.data) {
						const clip = data.data.clip as ClipInfo;
						info('ClipQueue: Clip added', clip);
						// Note: We rely on FullStateUpdate for actual state changes
					}
					break;

				case 'ClipRemoved':
					if (
						'data' in data &&
						data.data &&
						typeof data.data === 'object' &&
						'video_id' in data.data
					) {
						const videoId = data.data.video_id as string;
						info('ClipQueue: Clip removed', videoId);
						// Note: We rely on FullStateUpdate for actual state changes
					}
					break;

				case 'PlaybackStarted':
					if (
						'data' in data &&
						data.data &&
						typeof data.data === 'object' &&
						'video_id' in data.data
					) {
						const videoId = data.data.video_id as string;
						info('ClipQueue: Playback started', videoId);
						// Trigger YouTube player to start playing this video
						// This will be handled by the YouTube integration
					}
					break;

				case 'PlaybackStopped':
					info('ClipQueue: Playback stopped');
					// Trigger YouTube player to stop
					// This will be handled by the YouTube integration
					break;

				case 'ClipSubmissionRejected':
					if ('data' in data && data.data && typeof data.data === 'object') {
						const rejectionData = data.data as {
							submitted_by_username: string;
							input_text: string;
							reason: string;
						};
						warn('ClipQueue: Clip submission rejected', rejectionData);
						// Could show a notification here
					}
					break;

				case 'ConfigError':
					if (
						'data' in data &&
						data.data &&
						typeof data.data === 'object' &&
						'message' in data.data
					) {
						const message = data.data.message as string;
						error('ClipQueue: Config error', message);
						// Could show an error notification here
					}
					break;

				default:
					info('ClipQueue: Unhandled event type', data);
					break;
			}
		} else {
			warn('ClipQueue: Event missing event_type', data);
		}
	}

	// Computed values
	const computed = {
		get currentlyPlayingClip(): ClipInfo | null {
			if (gameState.phase?.name === 'Playing') {
				const currentClipVideoId = (gameState.phase as any).current_clip_video_id;
				return gameState.clipQueue?.find((clip) => clip.videoId === currentClipVideoId) || null;
			}
			return null;
		},

		get nextClipInQueue(): ClipInfo | null {
			if (gameState.phase?.name === 'Playing') {
				const currentClipVideoId = (gameState.phase as any).current_clip_video_id;
				const currentIndex =
					gameState.clipQueue?.findIndex((clip) => clip.videoId === currentClipVideoId) ?? -1;
				if (currentIndex !== -1 && currentIndex < (gameState.clipQueue?.length || 0) - 1) {
					return gameState.clipQueue?.[currentIndex + 1] || null;
				}
			} else if ((gameState.clipQueue?.length || 0) > 0) {
				return gameState.clipQueue?.[0] || null;
			}
			return null;
		},

		get queueCount(): number {
			return gameState.clipQueue?.length || 0;
		},

		get isPlaying(): boolean {
			return gameState.phase?.name === 'Playing';
		}
	};

	// Streaming methods
	function getPublicState(): ClipQueuePublicState {
		// Create a safe, serializable object (no getters/functions)
		const currentlyPlayingClip = computed.currentlyPlayingClip;
		const queueCount = computed.queueCount;

		return {
			phase: {
				type: gameState.phase?.name || 'Idle',
				currentClipVideoId:
					gameState.phase?.name === 'Playing'
						? (gameState.phase as any).current_clip_video_id
						: undefined
			},
			currentClip: currentlyPlayingClip ? { ...currentlyPlayingClip } : null,
			queueCount: queueCount,
			settings: gameState.settings
				? { ...gameState.settings }
				: {
						submissionsOpen: true,
						allowDuplicates: false,
						maxClipDurationSeconds: 600
					}
		};
	}

	function getStreamEvents(): StreamEvent[] {
		return [...streamEvents];
	}

	function clearStreamEvents(): void {
		streamEvents.length = 0;
	}

	function shouldBroadcastUpdate(): boolean {
		return false; // ClipQueue doesn't need broadcasting - admin and stream views are the same
	}

	// Register with game event router
	registerGameStore(GAME_TYPE_ID, {
		processEvent,
		getPublicState,
		getStreamEvents,
		clearStreamEvents,
		shouldBroadcastUpdate
	});

	return {
		get state() {
			return gameState;
		},
		actions,
		processEvent,
		computed,
		getPublicState,
		getStreamEvents,
		clearStreamEvents,
		shouldBroadcastUpdate
	};
}

export const clipQueueStore = createClipQueueStore();
