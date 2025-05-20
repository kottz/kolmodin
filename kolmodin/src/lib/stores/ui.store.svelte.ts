import { info } from '$lib/utils/logger';

export type AppScreen = 'home' | 'selectGame' | 'gameActive';

interface UiStoreState {
	currentScreen: AppScreen;
	activeGameTypeIdForUI: string | null; // To inform GameScreen.svelte which game component to load
	// We removed twitchChannelInput from here as it's local to SelectGameScreen
}

function createUiStore() {
	const state = $state<UiStoreState>({
		currentScreen: 'home',
		activeGameTypeIdForUI: null
	});

	function navigateToHome(): void {
		info('UI Store: Navigating to Home screen.');
		state.currentScreen = 'home';
		state.activeGameTypeIdForUI = null;
		// Potentially reset other UI-related states if necessary
	}

	function navigateToSelectGame(): void {
		info('UI Store: Navigating to Select Game screen.');
		state.currentScreen = 'selectGame';
		state.activeGameTypeIdForUI = null;
	}

	function navigateToGameActive(gameTypeId: string): void {
		if (!gameTypeId) {
			info('UI Store: Cannot navigate to Game Active without gameTypeId. Navigating home instead.');
			navigateToHome();
			return;
		}
		info(`UI Store: Navigating to Game Active screen for game type: ${gameTypeId}.`);
		state.currentScreen = 'gameActive';
		state.activeGameTypeIdForUI = gameTypeId;
	}

	// Function to call when leaving a game or disconnecting, to reset UI to home
	function resetToHomeState(): void {
		info('UI Store: Resetting to home state.');
		state.currentScreen = 'home';
		state.activeGameTypeIdForUI = null;
		// Any other UI cleanup specific to being in a game can go here.
	}

	return {
		get state() {
			return state;
		},
		navigateToHome,
		navigateToSelectGame,
		navigateToGameActive,
		resetToHomeState
	};
}

export const uiStore = createUiStore();
