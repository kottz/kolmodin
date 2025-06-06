// src/lib/types/stream.types.ts

// Stream event for animations and special notifications
export interface StreamEvent {
	type: string;
	data: any;
	duration?: number; // How long to show this event (ms)
	timestamp?: number;
}

// Base broadcast message structure
interface BaseBroadcastMessage {
	timestamp: number;
}

// State update message
export interface StateUpdateMessage extends BaseBroadcastMessage {
	type: 'STATE_UPDATE';
	gameType: string;
	state: any; // The public state for the specific game
}

// Stream event message (for animations, notifications, etc.)
export interface StreamEventMessage extends BaseBroadcastMessage {
	type: 'STREAM_EVENT';
	gameType: string;
	event: StreamEvent;
}

// Game changed message
export interface GameChangedMessage extends BaseBroadcastMessage {
	type: 'GAME_CHANGED';
	gameType: string | null;
}

// Stream control message
export interface StreamControlMessage extends BaseBroadcastMessage {
	type: 'STREAM_CONTROL';
	command: 'show' | 'hide' | 'clear';
}

// Union type for all broadcast messages
export type BroadcastMessage =
	| StateUpdateMessage
	| StreamEventMessage
	| GameChangedMessage
	| StreamControlMessage;

// Interface that game stores should implement for streaming
export interface StreamableGameStore {
	// Get the public state safe for streaming
	getPublicState(): any;

	// Get any pending stream events
	getStreamEvents?(): StreamEvent[];

	// Check if an update should be broadcast
	shouldBroadcastUpdate?(): boolean;

	// Clear any accumulated stream events
	clearStreamEvents?(): void;
}

// Generic public game state structure (games can extend this)
export interface BasePublicGameState {
	phase: any; // Game phase info (usually safe to share)
	// Games will add their own public fields
}

// Stream window state
export interface StreamWindowState {
	isVisible: boolean;
	currentGameType: string | null;
	gameState: any | null;
	activeEvents: StreamEvent[];
	lastUpdateTimestamp: number;
}

// Stream display configuration
export interface StreamDisplayConfig {
	showPlayerNames: boolean;
	showScores: boolean;
	showTimer: boolean;
	showPhase: boolean;
	animationDuration: number;
	maxActiveEvents: number;
}

// Window management types
export interface WindowPosition {
	x: number;
	y: number;
	width: number;
	height: number;
}

export interface StreamWindowOptions {
	position?: Partial<WindowPosition>;
	alwaysOnTop?: boolean;
	resizable?: boolean;
	title?: string;
}
