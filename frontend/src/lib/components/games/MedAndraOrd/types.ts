import type { BasePublicGameState } from '$lib/types/stream.types';

export interface AdminCommand {
	command:
		| 'StartGame'
		| 'PassWord'
		| 'ResetGame'
		| 'SetTargetPoints'
		| 'SetGameDuration'
		| 'SetPointLimitEnabled'
		| 'SetTimeLimitEnabled';
	points?: number; // For SetTargetPoints command
	seconds?: number; // For SetGameDuration command
	enabled?: boolean; // For SetPointLimitEnabled and SetTimeLimitEnabled commands
}

export type GamePhaseType =
	| { type: 'Setup' }
	| { type: 'Playing'; data: { current_word: string } }
	| { type: 'GameOver'; data: { winner: string } };

export interface MedAndraOrdGameState {
	phase: GamePhaseType;
	target_points: number;
	game_duration_seconds: number; // Changed from round_duration_seconds - total game duration
	point_limit_enabled: boolean;
	time_limit_enabled: boolean;
	player_scores: Record<string, number>;
}

// Public state interface for streaming (safe to broadcast)
export interface MedAndraOrdPublicState extends BasePublicGameState {
	phase: { type: string; data?: any };
	targetPoints: number;
	gameDurationSeconds: number;
	pointLimitEnabled: boolean;
	timeLimitEnabled: boolean;
	leaderboard: Array<{ player: string; points: number; rank: number }>;
	playersCount: number;
	timeRemaining?: number; // Only when game is active and time limit enabled
}

export type GameEventData =
	| { event_type: 'FullStateUpdate'; data: MedAndraOrdGameState }
	| { event_type: 'WordChanged'; data: { word: string } }
	| { event_type: 'PlayerScored'; data: { player: string; points: number } }
	| { event_type: 'GamePhaseChanged'; data: { new_phase: GamePhaseType } }
	| { event_type: 'GameTimeUpdate'; data: { remaining_seconds: number } }; // Optional: for server time sync

export type MedAndraOrdCommandData = AdminCommand;
