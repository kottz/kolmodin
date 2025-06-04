// src/lib/stores/ui.store.svelte.ts

import { info } from '$lib/utils/logger';
import { gameEventRouter } from '$lib/services/game.event.router';

export type AppScreen = 'home' | 'selectGame' | 'gameActive';

interface UiStoreState {
	currentScreen: AppScreen;
	activeGameTypeIdForUI: string | null; // To inform GameScreen.svelte which game component to load
}

interface StreamWindowInfo {
	window: Window | null;
	isOpen: boolean;
}

function createUiStore() {
	const state = $state<UiStoreState>({
		currentScreen: 'home',
		activeGameTypeIdForUI: null
	});

	const streamWindow = $state<StreamWindowInfo>({
		window: null,
		isOpen: false
	});

	function navigateToHome(): void {
		info('UI Store: Navigating to Home screen.');

		// Clear active game when going home
		if (state.activeGameTypeIdForUI) {
			gameEventRouter.setActiveGame(null);
		}

		state.currentScreen = 'home';
		state.activeGameTypeIdForUI = null;
	}

	function navigateToSelectGame(): void {
		info('UI Store: Navigating to Select Game screen.');

		// Clear active game when going to select screen
		if (state.activeGameTypeIdForUI) {
			gameEventRouter.setActiveGame(null);
		}

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

		// Set this as the active game for broadcasting
		gameEventRouter.setActiveGame(gameTypeId);
	}

	function resetToHomeState(): void {
		info('UI Store: Resetting to home state.');

		// Clear active game
		if (state.activeGameTypeIdForUI) {
			gameEventRouter.setActiveGame(null);
		}

		state.currentScreen = 'home';
		state.activeGameTypeIdForUI = null;

		// Close stream window if open
		closeStreamWindow();
	}

	// Stream window management functions
	function openStreamWindow(): void {
		if (streamWindow.window && !streamWindow.window.closed) {
			// Window already open, just focus it
			streamWindow.window.focus();
			return;
		}

		try {
			const windowFeatures = [
				'width=1280',
				'height=720',
				'left=100',
				'top=100',
				'toolbar=no',
				'menubar=no',
				'scrollbars=no',
				'resizable=yes',
				'location=no',
				'directories=no',
				'status=no'
			].join(',');

			const newWindow = window.open('/stream', 'gameStream', windowFeatures);

			if (newWindow) {
				streamWindow.window = newWindow;
				streamWindow.isOpen = true;

				// Monitor when window is closed
				const checkClosed = setInterval(() => {
					if (newWindow.closed) {
						streamWindow.window = null;
						streamWindow.isOpen = false;
						clearInterval(checkClosed);
						info('UI Store: Stream window was closed');
					}
				}, 1000);

				info('UI Store: Stream window opened');
			} else {
				info('UI Store: Failed to open stream window (popup blocked?)');
			}
		} catch (error) {
			info('UI Store: Error opening stream window:', error);
		}
	}

	function closeStreamWindow(): void {
		if (streamWindow.window && !streamWindow.window.closed) {
			streamWindow.window.close();
		}
		streamWindow.window = null;
		streamWindow.isOpen = false;
		info('UI Store: Stream window closed');
	}

	function toggleStreamWindow(): void {
		if (streamWindow.isOpen && streamWindow.window && !streamWindow.window.closed) {
			closeStreamWindow();
		} else {
			openStreamWindow();
		}
	}

	// Derived state
	const canOpenStreamWindow = $derived(
		state.currentScreen === 'gameActive' && state.activeGameTypeIdForUI !== null
	);

	return {
		get state() {
			return state;
		},
		get streamWindow() {
			return streamWindow;
		},
		get canOpenStreamWindow() {
			return canOpenStreamWindow;
		},

		// Navigation
		navigateToHome,
		navigateToSelectGame,
		navigateToGameActive,
		resetToHomeState,

		// Stream window management
		openStreamWindow,
		closeStreamWindow,
		toggleStreamWindow
	};
}

export const uiStore = createUiStore();
