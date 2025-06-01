export interface AdminCommand {
	command: 'StartGame' | 'PassWord' | 'ResetGame' | 'SetTargetPoints';
	points?: number; // For SetTargetPoints command
}

export type GamePhaseType =
	| { type: 'Setup' }
	| { type: 'Playing'; data: { current_word: string } }
	| { type: 'GameOver'; data: { winner: string } };

export interface MedAndraOrdGameState {
	phase: GamePhaseType;
	target_points: number;
	player_scores: Record<string, number>;
	timer_seconds_remaining: number;
}

export type GameEventData =
	| { event_type: 'FullStateUpdate'; data: MedAndraOrdGameState }
	| { event_type: 'WordChanged'; data: { word: string } }
	| { event_type: 'PlayerScored'; data: { player: string; points: number } }
	| { event_type: 'GamePhaseChanged'; data: { new_phase: GamePhaseType } }
	| { event_type: 'TimerUpdate'; data: { seconds_remaining: number } };

export type MedAndraOrdCommandData = AdminCommand;
