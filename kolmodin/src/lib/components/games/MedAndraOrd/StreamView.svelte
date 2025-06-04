<script lang="ts">
	import { fade, fly, scale } from 'svelte/transition';
	import { quintOut } from 'svelte/easing';
	import type { MedAndraOrdPublicState } from './types';
	import type { StreamDisplayConfig } from '$lib/types/stream.types';

	interface Props {
		gameState: MedAndraOrdPublicState;
		displayConfig: StreamDisplayConfig;
	}

	let { gameState, displayConfig }: Props = $props();

	function formatTime(seconds: number): string {
		const mins = Math.floor(seconds / 60);
		const secs = seconds % 60;
		return `${mins}:${secs.toString().padStart(2, '0')}`;
	}

	function getPhaseTitle(phase: string): string {
		switch (phase) {
			case 'Setup':
				return 'Getting Ready...';
			case 'Playing':
				return 'Guess the Word!';
			case 'GameOver':
				return 'Game Complete!';
			default:
				return 'Med Andra Ord';
		}
	}

	function getPhaseEmoji(phase: string): string {
		switch (phase) {
			case 'Setup':
				return '⚙️';
			case 'Playing':
				return '🎯';
			case 'GameOver':
				return '🏆';
			default:
				return '🎮';
		}
	}

	function getRankEmoji(rank: number): string {
		switch (rank) {
			case 1:
				return '👑';
			case 2:
				return '🥈';
			case 3:
				return '🥉';
			default:
				return '🎮';
		}
	}

	// Derived states for animations
	const isPlaying = $derived(gameState.phase.type === 'Playing');
	const hasPlayers = $derived(gameState.leaderboard.length > 0);
	const topPlayer = $derived(gameState.leaderboard[0]);
	const isLowTime = $derived(
		gameState.timeRemaining !== undefined && gameState.timeRemaining <= 30
	);
</script>

<div
	class="stream-view-container relative h-screen overflow-hidden bg-gradient-to-br from-indigo-900 via-purple-900 to-pink-800"
>
	<!-- Animated Background Elements -->
	<div class="absolute inset-0 opacity-20">
		<div
			class="absolute top-1/4 left-1/4 h-64 w-64 animate-pulse rounded-full bg-blue-500 blur-3xl"
		></div>
		<div
			class="absolute right-1/4 bottom-1/3 h-96 w-96 animate-pulse rounded-full bg-purple-500 blur-3xl delay-1000"
		></div>
		<div
			class="absolute top-2/3 left-1/2 h-48 w-48 animate-pulse rounded-full bg-pink-500 blur-2xl delay-2000"
		></div>
	</div>

	<!-- Main Content -->
	<div class="relative z-10 flex h-full flex-col">
		<!-- Header -->
		<header class="px-6 py-8 text-center">
			<div
				class="mb-4 animate-bounce text-6xl"
				transition:scale={{ duration: 500, easing: quintOut }}
			>
				{getPhaseEmoji(gameState.phase.type)}
			</div>
			<h1
				class="mb-2 text-5xl font-bold tracking-wide text-white"
				transition:fade={{ duration: 300 }}
			>
				Med Andra Ord
			</h1>
			<h2 class="text-2xl font-medium text-white/90" transition:fly={{ y: 20, duration: 400 }}>
				{getPhaseTitle(gameState.phase.type)}
			</h2>

			{#if isPlaying && displayConfig.showPhase}
				<div
					class="mt-4 animate-pulse text-xl text-yellow-300"
					transition:fly={{ y: -20, duration: 300 }}
				>
					🎤 Listen for clues and guess in chat!
				</div>
			{/if}
		</header>

		<!-- Game Status Bar -->
		{#if displayConfig.showTimer && gameState.timeLimitEnabled && gameState.timeRemaining !== undefined}
			<div class="mb-6 px-8">
				<div
					class="mx-auto max-w-md rounded-2xl bg-black/30 p-4 backdrop-blur-sm"
					transition:fly={{ y: -30, duration: 400 }}
				>
					<div class="text-center">
						<div class="mb-1 text-sm text-white/70">Time Remaining</div>
						<div
							class="font-mono text-4xl font-bold transition-colors duration-300"
							class:text-white={!isLowTime}
							class:text-red-400={isLowTime}
							class:animate-pulse={isLowTime}
						>
							{formatTime(gameState.timeRemaining)}
						</div>
					</div>
				</div>
			</div>
		{/if}

		<!-- Main Content Area -->
		<div class="flex-1 px-8 pb-8">
			{#if gameState.phase.type === 'Setup'}
				<!-- Setup Phase -->
				<div
					class="flex h-full flex-col items-center justify-center text-center"
					transition:fade={{ duration: 500 }}
				>
					<div class="max-w-2xl rounded-3xl bg-black/20 p-12 backdrop-blur-sm">
						<div class="mb-6 animate-spin text-7xl">⚙️</div>
						<h3 class="mb-4 text-3xl font-bold text-white">Setting Up Game</h3>
						<div class="space-y-2 text-lg text-white/80">
							{#if displayConfig.showScores && gameState.pointLimitEnabled}
								<p>🎯 Target: {gameState.targetPoints} points</p>
							{/if}
							{#if displayConfig.showTimer && gameState.timeLimitEnabled}
								<p>⏱️ Duration: {formatTime(gameState.gameDurationSeconds)}</p>
							{/if}
						</div>
						<div class="mt-6 text-white/60">
							<p>Get ready to guess words from the clues!</p>
						</div>
					</div>
				</div>
			{:else if gameState.phase.type === 'Playing'}
				<!-- Playing Phase -->
				<div class="grid h-full grid-cols-1 gap-8 lg:grid-cols-2">
					<!-- Leaderboard -->
					{#if displayConfig.showScores && hasPlayers}
						<div
							class="rounded-3xl bg-black/20 p-6 backdrop-blur-sm"
							transition:fly={{ x: -50, duration: 500 }}
						>
							<h3
								class="mb-6 flex items-center justify-center gap-2 text-center text-2xl font-bold text-white"
							>
								<span>🏆</span>
								Leaderboard
								<span>🏆</span>
							</h3>
							<div class="max-h-96 space-y-3 overflow-y-auto">
								{#each gameState.leaderboard.slice(0, 8) as { player, points, rank }, index (player)}
									<div
										class="rounded-xl p-4 backdrop-blur-sm transition-all duration-300 hover:bg-white/20 {rank ===
										1
											? 'bg-yellow-500/30'
											: rank === 2
												? 'bg-gray-400/30'
												: rank === 3
													? 'bg-amber-600/30'
													: 'bg-white/10'}"
										transition:fly={{ x: -30, duration: 300, delay: index * 100 }}
									>
										<div class="flex items-center justify-between">
											<div class="flex items-center gap-3">
												<span class="text-2xl">{getRankEmoji(rank)}</span>
												<div>
													<div class="text-lg font-bold text-white">
														{player}
													</div>
													<div class="text-sm text-white/70">
														Rank #{rank}
													</div>
												</div>
											</div>
											<div class="text-right">
												<div class="text-2xl font-bold text-white">
													{points}
												</div>
												<div class="text-sm text-white/70">
													{points === 1 ? 'point' : 'points'}
												</div>
											</div>
										</div>
									</div>
								{/each}
							</div>
						</div>
					{/if}

					<!-- Game Info & Status -->
					<div
						class="flex flex-col justify-center rounded-3xl bg-black/20 p-6 backdrop-blur-sm"
						transition:fly={{ x: 50, duration: 500 }}
					>
						<div class="space-y-6 text-center">
							<div class="animate-bounce text-6xl">🤔</div>
							<h3 class="text-3xl font-bold text-white">Listen Carefully!</h3>
							<div class="space-y-3 text-lg text-white/90">
								<p>💭 The admin is describing a word</p>
								<p>🎤 Type your guess in chat</p>
								<p>⚡ First correct answer wins a point!</p>
							</div>

							{#if displayConfig.showScores && gameState.pointLimitEnabled}
								<div class="rounded-xl bg-white/10 p-4">
									<div class="text-sm text-white/70">Target Score</div>
									<div class="text-3xl font-bold text-yellow-300">
										{gameState.targetPoints} points
									</div>
								</div>
							{/if}

							{#if displayConfig.showPlayerNames}
								<div class="text-sm text-white/60">
									{gameState.playersCount}
									{gameState.playersCount === 1 ? 'player' : 'players'} in game
								</div>
							{/if}
						</div>
					</div>
				</div>
			{:else if gameState.phase.type === 'GameOver'}
				<!-- Game Over Phase -->
				<div
					class="flex h-full flex-col items-center justify-center text-center"
					transition:fade={{ duration: 500 }}
				>
					<div class="max-w-4xl rounded-3xl bg-black/20 p-12 backdrop-blur-sm">
						<div class="mb-6 animate-bounce text-8xl">🎉</div>
						<h3 class="mb-6 text-4xl font-bold text-white">Game Complete!</h3>

						{#if topPlayer && displayConfig.showScores}
							<div
								class="mb-6 rounded-2xl bg-gradient-to-r from-yellow-400/20 to-yellow-600/20 p-6"
								transition:scale={{ duration: 500, delay: 200 }}
							>
								<div class="mb-4 text-6xl">👑</div>
								<h4 class="mb-2 text-2xl font-bold text-yellow-300">
									Winner: {topPlayer.player}
								</h4>
								<div class="text-xl text-white/90">
									{topPlayer.points}
									{topPlayer.points === 1 ? 'point' : 'points'}
								</div>
							</div>
						{/if}

						{#if displayConfig.showScores && gameState.leaderboard.length > 1}
							<div class="text-lg text-white/80">
								<h5 class="mb-3 font-semibold">Final Standings:</h5>
								<div class="grid grid-cols-1 gap-3 md:grid-cols-2">
									{#each gameState.leaderboard.slice(0, 6) as { player, points, rank }, index}
										<div
											class="flex items-center justify-between rounded-lg bg-white/10 p-3"
											transition:fly={{ y: 20, duration: 300, delay: index * 100 }}
										>
											<span class="flex items-center gap-2">
												<span>{getRankEmoji(rank)}</span>
												{player}
											</span>
											<span class="font-bold">{points}</span>
										</div>
									{/each}
								</div>
							</div>
						{/if}

						<div class="mt-8 text-white/60">
							<p>Thanks for playing Med Andra Ord!</p>
						</div>
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

	/* Custom scrollbar for leaderboard */
	.overflow-y-auto::-webkit-scrollbar {
		width: 8px;
	}

	.overflow-y-auto::-webkit-scrollbar-track {
		background: rgba(255, 255, 255, 0.1);
		border-radius: 4px;
	}

	.overflow-y-auto::-webkit-scrollbar-thumb {
		background: rgba(255, 255, 255, 0.3);
		border-radius: 4px;
	}

	.overflow-y-auto::-webkit-scrollbar-thumb:hover {
		background: rgba(255, 255, 255, 0.5);
	}
</style>
