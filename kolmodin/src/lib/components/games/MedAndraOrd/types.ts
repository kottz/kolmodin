export interface AdminCommand {
	command:
		| 'StartGame'
		| 'PassWord'
		| 'ResetGame'
		| 'SetTargetPoints'
		| 'SetGameTimeLimit'
		| 'SetPointLimitEnabled'
		| 'SetTimeLimitEnabled';
	points?: number; // For SetTargetPoints command
	minutes?: number; // For SetGameTimeLimit command
	enabled?: boolean; // For SetPointLimitEnabled and SetTimeLimitEnabled commands
}

export type GamePhaseType =
	| { type: 'Setup' }
	| { type: 'Playing'; data: { current_word: string } }
	| { type: 'GameOver'; data: { winner?: string; reason: 'points' | 'time' } };

export interface MedAndraOrdGameState {
	phase: GamePhaseType;
	target_points: number;
	game_time_limit_minutes: number;
	point_limit_enabled: boolean;
	time_limit_enabled: boolean;
	player_scores: Record<string, number>;
	round_duration_seconds: number; // Always 60, not configurable
}

export type GameEventData =
	| { event_type: 'FullStateUpdate'; data: MedAndraOrdGameState }
	| { event_type: 'WordChanged'; data: { word: string } }
	| { event_type: 'PlayerScored'; data: { player: string; points: number } }
	| { event_type: 'GamePhaseChanged'; data: { new_phase: GamePhaseType } }
	| { event_type: 'GameTimeUpdate'; data: { seconds_remaining: number } };

export type MedAndraOrdCommandData = AdminCommand;
