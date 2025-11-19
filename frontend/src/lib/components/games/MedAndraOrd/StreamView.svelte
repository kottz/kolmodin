<script lang="ts">
	import { fly, scale } from 'svelte/transition';
	import { elasticOut } from 'svelte/easing';
	import { onDestroy } from 'svelte';
	import type { MedAndraOrdPublicState, RecentGuess } from './types';

	interface Props {
		gameState: MedAndraOrdPublicState;
	}

	let { gameState }: Props = $props();

	// Get stream events from the stream store
	import { streamStore } from '$lib/stores/stream.store.svelte';

	// Track guess history (now using server's recent guesses)
	interface GuessHistoryEntry {
		id: string;
		player: string;
		guess: string;
		isCorrect: boolean;
		timestamp: number;
	}

	// Local state for recent guesses (updated via stream events)
	let recentGuesses = $state<RecentGuess[]>([]);

	// Convert recent guesses to display format
	const displayGuesses = $derived(() => {
		return recentGuesses.map(
			(guess): GuessHistoryEntry => ({
				id: guess.id,
				player: guess.player,
				guess: guess.guessed_text,
				isCorrect: true, // All recent guesses are correct
				timestamp: guess.timestamp * 1000 // Convert to milliseconds
			})
		);
	});

	// Track current instruction card state
	let instructionState = $state<{
		text: string;
		type: 'instruction' | 'correct-answer' | 'game-over';
		player?: string;
		word?: string;
		winner?: string;
	}>({ text: 'Listen carefully and guess in chat!', type: 'instruction' });

	// Track game over animation state
	let gameOverAnimationState = $state<'none' | 'celebration' | 'leaderboard'>('none');

	// Stream view timer (independent from admin view)
	let streamTimer = $state<number>(0);
	let streamTimerInterval: number | null = null;
	let timerStarted = $state<boolean>(false);

	// Track previous leaderboard for animations - simple state tracking
	let previousLeaderboard = $state<typeof gameState.leaderboard>([]);
	let scoringPlayers = $state<Set<string>>(new Set());

	// Track processed events to prevent duplicates
	let processedEventIds = new Set<string>();

	// Simple function to check for scoring changes
	function checkForScoringChanges() {
		if (gameState.leaderboard.length > 0 && previousLeaderboard.length > 0) {
			const newScoringPlayers = new Set<string>();

			gameState.leaderboard.forEach((current) => {
				const previous = previousLeaderboard.find((p) => p.player === current.player);
				if (previous && current.points > previous.points) {
					newScoringPlayers.add(current.player);
				}
			});

			if (newScoringPlayers.size > 0) {
				scoringPlayers = newScoringPlayers;

				setTimeout(() => {
					scoringPlayers = new Set();
				}, 2000);
			}
		}

		previousLeaderboard = [...gameState.leaderboard];
	}

	// Manual event processing instead of reactive $effect
	function processStreamEvents() {
		const events = streamStore.state.activeEvents;

		events.forEach((event) => {
			const eventId = `${event.type}-${event.timestamp}`;

			// Skip if already processed
			if (processedEventIds.has(eventId)) return;

			if (event.type === 'CORRECT_ANSWER' && event.data.player && event.data.word) {
				showCorrectAnswer(event.data.player, event.data.word);
				processedEventIds.add(eventId);
			} else if (event.type === 'RECENT_GUESSES_UPDATED' && event.data.recentGuesses) {
				recentGuesses = event.data.recentGuesses;
				processedEventIds.add(eventId);
			}
		});

		// Clean up old processed event IDs (keep last 20)
		if (processedEventIds.size > 20) {
			const idsArray = Array.from(processedEventIds);
			processedEventIds = new Set(idsArray.slice(-20));
		}
	}

	function showCorrectAnswer(player: string, word: string) {
		// Update instruction card
		instructionState = {
			text: `${player} was correct!`,
			type: 'correct-answer',
			player,
			word
		};

		// Clear any existing timeout
		if (instructionTimeout) clearTimeout(instructionTimeout);

		// Reset to instruction after longer delay
		instructionTimeout = setTimeout(() => {
			instructionState = {
				text: 'Listen carefully and guess in chat!',
				type: 'instruction'
			};
			instructionTimeout = null;
		}, 6000);
	}

	function formatTime(seconds: number): string {
		const mins = Math.floor(seconds / 60);
		const secs = seconds % 60;
		return `${mins}:${secs.toString().padStart(2, '0')}`;
	}

	// Stream timer management
	function startStreamTimer() {
		if (timerStarted || !gameState.timeLimitEnabled) return;

		stopStreamTimer();
		streamTimer = gameState.gameDurationSeconds;
		timerStarted = true;

		streamTimerInterval = setInterval(() => {
			streamTimer--;
			if (streamTimer <= 0) {
				stopStreamTimer();
			}
		}, 1000);
	}

	function stopStreamTimer() {
		if (streamTimerInterval) {
			clearInterval(streamTimerInterval);
			streamTimerInterval = null;
		}
	}

	function resetStreamTimer() {
		stopStreamTimer();
		timerStarted = false;
		streamTimer = gameState.gameDurationSeconds;
	}

	// Handle game over animation sequence
	function startGameOverSequence() {
		const winner = gameState.leaderboard[0]?.player;
		console.log('Starting game over sequence, winner:', winner);
		console.log('Leaderboard:', gameState.leaderboard);

		// Start with celebration animation
		gameOverAnimationState = 'celebration';

		// Clear any existing timeout
		if (gameOverTimeout) clearTimeout(gameOverTimeout);

		// After 3 seconds, slide in leaderboard and update instruction card
		gameOverTimeout = setTimeout(() => {
			console.log('Moving to leaderboard phase');
			gameOverAnimationState = 'leaderboard';
			instructionState = {
				text: winner ? `🏆 ${winner} Wins! 🏆` : 'Game Complete!',
				type: 'game-over',
				winner
			};
			gameOverTimeout = null;
		}, 3000);
	}

	// Reset state when game phase changes
	function handlePhaseChange() {
		if (gameState.phase.type === 'Setup') {
			processedEventIds.clear();
			gameOverAnimationState = 'none';
			instructionState = { text: 'Listen carefully and guess in chat!', type: 'instruction' };
			recentGuesses = []; // Clear recent guesses on setup
			resetStreamTimer();
		} else if (gameState.phase.type === 'Playing') {
			// Start the stream timer when game starts
			startStreamTimer();
		} else if (gameState.phase.type === 'GameOver') {
			// Stop timer and start game over sequence
			stopStreamTimer();
			if (gameOverAnimationState === 'none') {
				startGameOverSequence();
			}
		}
	}

	// Watch for phase changes and process events manually
	let lastPhase = gameState.phase.type;
	let lastEventCount = 0;

	// Manual update check (called periodically or on prop changes)
	function updateCheck() {
		// Check for phase changes
		if (lastPhase !== gameState.phase.type) {
			console.log(`Phase changed from ${lastPhase} to ${gameState.phase.type}`);
			lastPhase = gameState.phase.type;
			handlePhaseChange();
		}

		// Check for new events
		const currentEventCount = streamStore.state.activeEvents.length;
		if (currentEventCount !== lastEventCount) {
			lastEventCount = currentEventCount;
			processStreamEvents();
		}

		// Check for scoring changes
		checkForScoringChanges();
	}

	// Use a simple interval to check for updates instead of reactive effects
	let updateInterval: number;
	updateInterval = setInterval(updateCheck, 200); // Check every 200ms for faster detection

	// Also add a reactive check as backup using $derived
	const currentPhase = $derived(gameState.phase.type);

	// Simple reactive backup for phase changes
	$effect(() => {
		if (currentPhase !== lastPhase) {
			console.log(`Reactive phase change detected: ${lastPhase} -> ${currentPhase}`);
			lastPhase = currentPhase;
			handlePhaseChange();
		}
	});

	// Derived states
	const isPlaying = $derived(gameState.phase.type === 'Playing');
	const isLowTime = $derived(streamTimer <= 30 && streamTimer > 0);
	const showTimer = $derived(gameState.timeLimitEnabled && isPlaying);
	const showPointLimit = $derived(gameState.pointLimitEnabled && gameState.targetPoints > 0);

	// Cleanup timeouts on destroy
	let instructionTimeout: number | null = null;
	let gameOverTimeout: number | null = null;

	onDestroy(() => {
		if (instructionTimeout) clearTimeout(instructionTimeout);
		if (gameOverTimeout) clearTimeout(gameOverTimeout);
		if (updateInterval) clearInterval(updateInterval);
		stopStreamTimer();
	});
</script>

<div class="stream-view-container relative h-screen overflow-hidden bg-slate-950">
	<!-- Subtle background gradient -->
	<div class="absolute inset-0 bg-linear-to-br from-slate-900 to-slate-950 opacity-80"></div>

	<!-- Main Content -->
	<div class="relative z-10 flex h-full flex-col">
		<!-- Top Instruction Card - Hidden during final leaderboard -->
		{#if !(gameState.phase.type === 'GameOver' && gameOverAnimationState === 'leaderboard')}
			<div class="p-6 pb-4">
				<div class="rounded-2xl border border-white/10 bg-black/20 px-8 py-6 backdrop-blur-sm">
					<div class="flex items-center justify-between">
						<!-- Left side: Timer and Target Score -->
						<div class="flex gap-4">
							{#if showTimer}
								<div
									class="rounded-xl border border-white/10 bg-black/20 px-4 py-3 backdrop-blur-sm"
								>
									<div class="text-center">
										<div class="mb-1 text-xs text-white/70">Time Remaining</div>
										<div
											class="font-mono text-xl font-bold transition-colors duration-300"
											class:text-white={!isLowTime}
											class:text-red-400={isLowTime}
											class:animate-pulse={isLowTime}
										>
											{formatTime(streamTimer)}
										</div>
									</div>
								</div>
							{/if}

							{#if showPointLimit}
								<div
									class="rounded-xl border border-white/10 bg-black/20 px-4 py-3 backdrop-blur-sm"
								>
									<div class="text-center">
										<div class="mb-1 text-xs text-white/70">Target Score</div>
										<div class="text-xl font-bold text-yellow-400">
											{gameState.targetPoints}
										</div>
									</div>
								</div>
							{/if}
						</div>

						<!-- Center: Main content -->
						<div class="flex-1 text-center">
							{#if instructionState.type === 'correct-answer'}
								<div class="space-y-2">
									<div class="text-2xl font-semibold text-white">
										{instructionState.text}
									</div>
									{#if instructionState.word}
										<div
											class="text-4xl font-bold tracking-wider text-green-400"
											transition:scale={{ duration: 600, easing: elasticOut }}
										>
											{instructionState.word}
										</div>
									{/if}
								</div>
							{:else if instructionState.type === 'game-over'}
								<div class="space-y-2">
									<div
										class="text-4xl font-bold text-yellow-400"
										transition:scale={{ duration: 600, easing: elasticOut }}
									>
										{instructionState.text}
									</div>
									{#if instructionState.winner}
										<div class="text-lg text-white/80">
											Final Score: {gameState.leaderboard[0]?.points} points
										</div>
									{/if}
								</div>
							{:else}
								<div class="space-y-2">
									<div class="text-3xl font-bold text-white">Med Andra Ord</div>
									<div class="text-lg text-white/80">{instructionState.text}</div>
								</div>
							{/if}
						</div>

						<!-- Right side: Empty for balance -->
						<div class="flex gap-4 opacity-0">
							{#if showTimer}
								<div
									class="rounded-xl border border-white/10 bg-black/20 px-4 py-3 backdrop-blur-sm"
								>
									<div class="text-center">
										<div class="mb-1 text-xs text-white/70">Time Remaining</div>
										<div class="font-mono text-xl font-bold">00:00</div>
									</div>
								</div>
							{/if}
							{#if showPointLimit}
								<div
									class="rounded-xl border border-white/10 bg-black/20 px-4 py-3 backdrop-blur-sm"
								>
									<div class="text-center">
										<div class="mb-1 text-xs text-white/70">Target Score</div>
										<div class="text-xl font-bold">0</div>
									</div>
								</div>
							{/if}
						</div>
					</div>
				</div>
			</div>
		{/if}

		<!-- Main Game Area -->
		<div
			class={gameState.phase.type === 'GameOver' && gameOverAnimationState === 'leaderboard'
				? 'flex-1 px-6 pb-6'
				: 'flex flex-1 gap-6 px-6 pb-6'}
		>
			{#if gameState.phase.type === 'GameOver'}
				<!-- Game Over States -->
				{#if gameOverAnimationState === 'celebration'}
					<!-- Game Over Celebration Animation -->
					<div class="flex flex-1 items-center justify-center">
						<div class="text-center" transition:scale={{ duration: 1000, easing: elasticOut }}>
							<div class="mb-6 animate-bounce text-9xl">🎉</div>
							<h1 class="mb-6 animate-pulse text-6xl font-bold text-white">Game Over!</h1>
							{#if gameState.leaderboard[0]}
								<div class="space-y-2">
									<div class="text-4xl font-bold text-yellow-400">
										🏆 {gameState.leaderboard[0].player} Wins! 🏆
									</div>
									<div class="text-2xl text-white/80">
										{gameState.leaderboard[0].points} points
									</div>
								</div>
							{/if}
						</div>
					</div>
				{:else if gameOverAnimationState === 'leaderboard'}
					<!-- Full Screen Final Leaderboard -->
					<div
						class="absolute inset-0 h-full w-full"
						style="left: 24px; right: 24px; bottom: 24px; top: 0; width: auto;"
						in:fly={{ x: -50, duration: 600, easing: elasticOut }}
					>
						<div class="h-full rounded-2xl border border-white/10 bg-black/20 p-8 backdrop-blur-sm">
							<h2 class="mb-8 text-center text-4xl font-bold text-white">🏆 Final Results 🏆</h2>
							<div class="mx-auto max-w-4xl space-y-4">
								{#each gameState.leaderboard as { player, points, rank }, index (player)}
									<div
										class="flex items-center justify-between rounded-xl p-6 transition-all duration-500 {rank ===
										1
											? 'border-2 border-yellow-500/50 bg-linear-to-r from-yellow-500/40 to-yellow-600/40'
											: rank === 2
												? 'border border-gray-400/40 bg-linear-to-r from-gray-400/30 to-gray-500/30'
												: rank === 3
													? 'border border-amber-600/40 bg-linear-to-r from-amber-600/30 to-amber-700/30'
													: 'border border-white/20 bg-white/10'}"
										transition:fly={{
											x: -100,
											duration: 600,
											delay: index * 200,
											easing: elasticOut
										}}
									>
										<div class="flex items-center gap-6">
											<div class="w-16 text-center">
												{#if rank === 1}
													<div class="text-6xl">👑</div>
												{:else if rank === 2}
													<div class="text-5xl">🥈</div>
												{:else if rank === 3}
													<div class="text-5xl">🥉</div>
												{:else}
													<div class="text-4xl font-bold text-white/60">#{rank}</div>
												{/if}
											</div>
											<div>
												<div class="text-3xl font-bold text-white">
													{player}
												</div>
												<div class="text-lg text-white/70">
													{rank === 1
														? 'Champion!'
														: rank === 2
															? 'Runner-up'
															: rank === 3
																? 'Third Place'
																: `${rank}th Place`}
												</div>
											</div>
										</div>
										<div class="text-right">
											<div class="text-4xl font-bold text-white">
												{points}
											</div>
											<div class="text-lg text-white/70">
												{points === 1 ? 'point' : 'points'}
											</div>
										</div>
									</div>
								{/each}

								{#if gameState.leaderboard.length === 0}
									<div class="flex h-32 items-center justify-center text-white/50">
										Waiting for players to join...
									</div>
								{/if}
							</div>
						</div>
					</div>
				{/if}
			{:else if isPlaying}
				<!-- Playing State: Split View -->
				<div class="w-1/2">
					<div class="h-full rounded-2xl border border-white/10 bg-black/20 p-6 backdrop-blur-sm">
						<h2 class="mb-6 text-center text-2xl font-bold text-white">Leaderboard</h2>
						<div class="space-y-3">
							{#each gameState.leaderboard as { player, points, rank }, index (player)}
								<div
									class="flex items-center justify-between rounded-xl p-4 transition-all duration-500 {rank ===
									1
										? 'bg-yellow-500/30'
										: rank === 2
											? 'bg-gray-400/20'
											: rank === 3
												? 'bg-amber-600/20'
												: 'bg-white/5'}"
									class:scale-105={scoringPlayers.has(player)}
									class:animate-pulse={scoringPlayers.has(player)}
									transition:fly={{ x: -50, duration: 300, delay: index * 50 }}
								>
									<div class="flex items-center gap-3">
										<div class="w-8 text-center text-2xl font-bold text-white/60">
											{rank}
										</div>
										<div class="truncate text-lg font-semibold text-white">
											{player}
										</div>
									</div>
									<div class="text-2xl font-bold text-white">
										{points}
									</div>
								</div>
							{/each}

							{#if gameState.leaderboard.length === 0}
								<div class="flex h-32 items-center justify-center text-white/50">
									Waiting for players to join...
								</div>
							{/if}
						</div>
					</div>
				</div>

				<!-- Right Side: Guess History -->
				<div class="w-1/2">
					<div class="h-full rounded-2xl border border-white/10 bg-black/20 p-6 backdrop-blur-sm">
						<h2 class="mb-6 text-center text-2xl font-bold text-white">Recent Guesses</h2>
						<div class="space-y-3">
							{#each displayGuesses() as guess, index (guess.id)}
								<div
									class="rounded-xl p-4 transition-all duration-300 {guess.isCorrect
										? 'border border-green-500/30 bg-green-500/20'
										: 'bg-white/5'}"
									transition:fly={{ x: 50, duration: 300, delay: index * 50 }}
								>
									<div class="flex items-center justify-between">
										<div class="flex-1">
											<div class="text-lg font-semibold text-white">
												{guess.player}
											</div>
											<div class="text-sm text-white/70">
												guessed "{guess.guess}"
											</div>
										</div>
										{#if guess.isCorrect}
											<div class="text-2xl text-green-400">✓</div>
										{/if}
									</div>
								</div>
							{/each}

							{#if displayGuesses().length === 0}
								<div class="flex h-32 items-center justify-center text-white/50">
									No guesses yet...
								</div>
							{/if}
						</div>
					</div>
				</div>
			{:else if gameState.phase.type === 'Setup'}
				<!-- Setup State -->
				<div class="flex flex-1 items-center justify-center">
					<div class="text-center">
						<div class="mb-4 text-6xl">⚙️</div>
						<h1 class="mb-2 text-4xl font-bold text-white">Setting up game...</h1>
					</div>
				</div>
			{:else}
				<!-- Unknown/Waiting State -->
				<div class="flex flex-1 items-center justify-center">
					<div class="text-center">
						<div class="mb-4 text-6xl">⏳</div>
						<h1 class="mb-2 text-4xl font-bold text-white">Waiting for game...</h1>
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>

<style>
	.stream-view-container {
		font-family:
			'Inter',
			-apple-system,
			BlinkMacSystemFont,
			sans-serif;
	}
</style>
