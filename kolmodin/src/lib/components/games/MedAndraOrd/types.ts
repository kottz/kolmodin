export interface AdminCommand {
	command:
		| 'StartGame'
		| 'PassWord'
		| 'ResetGame'
		| 'SetTargetPoints'
		| 'SetRoundDuration'
		| 'SetPointLimitEnabled'
		| 'SetTimeLimitEnabled';
	points?: number; // For SetTargetPoints command
	seconds?: number; // For SetRoundDuration command
	enabled?: boolean; // For SetPointLimitEnabled and SetTimeLimitEnabled commands
}

export type GamePhaseType =
	| { type: 'Setup' }
	| { type: 'Playing'; data: { current_word: string } }
	| { type: 'GameOver'; data: { winner: string } };

export interface MedAndraOrdGameState {
	phase: GamePhaseType;
	target_points: number;
	round_duration_seconds: number; // Configurable duration per word
	point_limit_enabled: boolean;
	time_limit_enabled: boolean;
	player_scores: Record<string, number>;
}

export type GameEventData =
	| { event_type: 'FullStateUpdate'; data: MedAndraOrdGameState }
	| { event_type: 'WordChanged'; data: { word: string } }
	| { event_type: 'PlayerScored'; data: { player: string; points: number } }
	| { event_type: 'GamePhaseChanged'; data: { new_phase: GamePhaseType } };

export type MedAndraOrdCommandData = AdminCommand;
