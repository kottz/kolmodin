export interface ClipInfo {
	videoId: string;
	title: string;
	channelTitle: string;
	durationIso8601: string;
	thumbnailUrl: string;
	submittedByUsername: string;
	submittedAtTimestamp: number;
}

export interface ClipQueueSettings {
	submissionsOpen: boolean;
	allowDuplicates: boolean;
	maxClipDurationSeconds: number;
}

export type ClipQueueGamePhase = { name: 'Idle' } | { name: 'Playing'; currentClipVideoId: string };

export interface ClipQueueFullState {
	phase: ClipQueueGamePhase;
	clipQueue: ClipInfo[];
	playedClipIds: string[];
	removedByAdminClipIds: string[];
	settings: ClipQueueSettings;
}

// Game event data types
export type ClipAddedEvent = { clip: ClipInfo };
export type ClipRemovedEvent = { videoId: string; removedByAdmin: boolean };
export type PlaybackStartedEvent = { videoId: string };
export type PlaybackStoppedEvent = {};
export type SettingsChangedEvent = { newSettings: ClipQueueSettings };
export type QueueWasResetEvent = {};
export type ClipSubmissionRejectedEvent = {
	submittedByUsername: string;
	inputText: string;
	reason: string;
};
export type ConfigErrorEvent = { message: string };
export type FullStateUpdateEvent = { state: ClipQueueFullState };

export type ClipQueueEventData =
	| ClipAddedEvent
	| ClipRemovedEvent
	| PlaybackStartedEvent
	| PlaybackStoppedEvent
	| SettingsChangedEvent
	| QueueWasResetEvent
	| ClipSubmissionRejectedEvent
	| ConfigErrorEvent
	| FullStateUpdateEvent;

// Admin command data types
export type PlayClipCommand = { videoId: string };
export type PlayNextInQueueCommand = {};
export type RemoveClipFromQueueCommand = { videoId: string };
export type UpdateSettingsCommand = { newSettings: ClipQueueSettings };
export type ResetQueueCommand = {};
export type NotifyClipFinishedPlayingCommand = { videoId: string };

export type ClipQueueCommandData =
	| PlayClipCommand
	| PlayNextInQueueCommand
	| RemoveClipFromQueueCommand
	| UpdateSettingsCommand
	| ResetQueueCommand
	| NotifyClipFinishedPlayingCommand;

// Utility function to convert ISO 8601 duration to human readable format
export function formatDuration(iso8601Duration: string): string {
	// Parse PT3M32S format
	const match = iso8601Duration.match(/PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?/);
	if (!match) return '0:00';

	const hours = parseInt(match[1] || '0');
	const minutes = parseInt(match[2] || '0');
	const seconds = parseInt(match[3] || '0');

	if (hours > 0) {
		return `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
	} else {
		return `${minutes}:${seconds.toString().padStart(2, '0')}`;
	}
}

// Utility function to get time ago string
export function getTimeAgo(timestamp: number): string {
	const now = Date.now() / 1000;
	const diff = now - timestamp;

	if (diff < 60) return 'just now';
	if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
	if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
	return `${Math.floor(diff / 86400)}d ago`;
}
