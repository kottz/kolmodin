<!-- src/lib/components/games/Quiz/AdminView.svelte -->
<script lang="ts">
	import { quizStore } from './store.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Checkbox from '$lib/components/ui/checkbox.svelte';
	import Label from '$lib/components/ui/label.svelte';
	import StreamControls from '$lib/components/ui/StreamControls.svelte';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card';
	import Badge from '$lib/components/ui/badge.svelte';
	import RecentGuesses from './RecentGuesses.svelte';

	const gameState = $derived(quizStore.gameState);
	const leaderboard = $derived(quizStore.leaderboard);
	const currentWord = $derived(quizStore.currentWord);
	const winner = $derived(quizStore.winner);
	const displayTimer = $derived(quizStore.displayTimer);
	const currentPhase = $derived(gameState.phase.type);

	let targetPointsInput = $state('10');
	let gameDurationInput = $state('300'); // 5 minutes default
	let pointLimitEnabled = $state(true);
	let timeLimitEnabled = $state(false);

	function handlePointLimitToggle() {
		quizStore.actions.setPointLimitEnabled(pointLimitEnabled);
	}

	function handleTimeLimitToggle() {
		quizStore.actions.setTimeLimitEnabled(timeLimitEnabled);
	}

	function handleStartGame() {
		// Apply current settings before starting the game
		if (pointLimitEnabled) {
			const points = parseInt(targetPointsInput);
			if (!isNaN(points) && points > 0) {
				quizStore.actions.setTargetPoints(points);
			}
		}

		if (timeLimitEnabled) {
			const seconds = parseInt(gameDurationInput);
			if (!isNaN(seconds) && seconds > 0) {
				quizStore.actions.setGameDuration(seconds);
			}
		}

		// Apply toggle states
		quizStore.actions.setPointLimitEnabled(pointLimitEnabled);
		quizStore.actions.setTimeLimitEnabled(timeLimitEnabled);

		// Start the game
		quizStore.actions.startGame();
	}

	function formatTime(seconds: number): string {
		const mins = Math.floor(seconds / 60);
		const secs = seconds % 60;
		return `${mins}:${secs.toString().padStart(2, '0')}`;
	}
</script>

<div class="bg-background relative min-h-screen">
	<div class="mx-auto mt-6 px-4 sm:px-6 lg:px-8">
		<!-- Game Status Card -->
		<div class="mb-6">
			<Card>
				<CardContent class="p-8 text-center">
					{#if currentPhase === 'Setup'}
						<h2 class="text-4xl font-bold">Quiz</h2>
						<p class="text-muted-foreground mt-2 text-lg">Ready to start the quiz game!</p>
						<p class="text-muted-foreground mt-1 text-sm">
							Configure settings and click Start Game
						</p>
					{:else if currentPhase === 'Playing'}
						<h2 class="mb-4 text-4xl font-bold">Current Question</h2>
						<div class="text-primary mb-4 font-mono text-6xl font-bold">
							{currentWord() || 'Loading...'}
						</div>
						{#if timeLimitEnabled}
							<div class="text-muted-foreground text-2xl">
								Game Time: {formatTime(displayTimer())}
							</div>
						{/if}
						<p class="text-muted-foreground mt-2 text-sm">
							Read this question to your Twitch chat! First correct answer wins a point.
						</p>
					{:else if currentPhase === 'GameOver'}
						<h2 class="text-4xl font-bold text-green-600">Quiz Over!</h2>
						<div class="mt-4 text-3xl font-bold">
							🎉 Winner: {winner()} 🎉
						</div>
						<p class="text-muted-foreground mt-2 text-lg">
							{#if gameState.point_limit_enabled && gameState.time_limit_enabled}
								Quiz ended - target reached or time expired!
							{:else if gameState.point_limit_enabled}
								Congratulations on reaching {gameState.target_points} points!
							{:else}
								Time's up! Final scores are shown below.
							{/if}
						</p>
					{/if}
				</CardContent>
			</Card>
		</div>

		<div class="grid grid-cols-1 gap-6 lg:grid-cols-3">
			<!-- Admin Controls -->
			<div class="space-y-6 lg:col-span-1">
				<Card>
					<CardHeader>
						<CardTitle>Admin Controls</CardTitle>
						<CardDescription>Control the quiz flow and settings</CardDescription>
					</CardHeader>
					<CardContent class="space-y-4">
						{#if currentPhase === 'Setup'}
							<!-- Point Limit Settings -->
							<div class="space-y-3">
								<div class="flex items-center gap-2">
									<Checkbox
										id="point-limit-enabled"
										bind:checked={pointLimitEnabled}
										onCheckedChange={handlePointLimitToggle}
									/>
									<Label for="point-limit-enabled">
										{#snippet children()}
											Point Limit
										{/snippet}
									</Label>
								</div>
								<div class="space-y-2">
									<Input
										id="target-points"
										type="number"
										min="1"
										max="50"
										bind:value={targetPointsInput}
										placeholder="10"
										disabled={!pointLimitEnabled}
									/>
								</div>
							</div>

							<!-- Time Limit Settings -->
							<div class="space-y-3">
								<div class="flex items-center gap-2">
									<Checkbox
										id="time-limit-enabled"
										bind:checked={timeLimitEnabled}
										onCheckedChange={handleTimeLimitToggle}
									/>
									<Label for="time-limit-enabled">
										{#snippet children()}
											Quiz Time Limit
										{/snippet}
									</Label>
								</div>
								<div class="space-y-2">
									<Input
										id="game-duration"
										type="number"
										min="60"
										max="1800"
										bind:value={gameDurationInput}
										placeholder="300"
										disabled={!timeLimitEnabled}
									/>
									<p class="text-muted-foreground text-xs">Duration in seconds (60-1800)</p>
								</div>
							</div>
							<Button onclick={handleStartGame} class="w-full" size="lg">Start Quiz</Button>
						{:else if currentPhase === 'Playing'}
							<div class="flex flex-col gap-2">
								<Button onclick={quizStore.actions.passWord} variant="outline" class="w-full">
									Skip Question
								</Button>
								<Button onclick={quizStore.actions.resetGame} variant="destructive" class="w-full">
									End Quiz
								</Button>
							</div>
						{:else if currentPhase === 'GameOver'}
							<Button onclick={quizStore.actions.resetGame} class="w-full" size="lg">
								New Quiz
							</Button>
						{/if}
					</CardContent>
				</Card>

				<!-- Stream Controls Card -->
				<StreamControls />

				<!-- Recent Guesses (only show during Playing phase) -->
				{#if currentPhase === 'Playing'}
					<RecentGuesses
						recentGuesses={gameState.recent_guesses}
						onRemoveGuess={quizStore.actions.removeRecentGuess}
					/>
				{/if}

				<!-- Game Info -->
				<Card>
					<CardHeader>
						<CardTitle>Quiz Info</CardTitle>
					</CardHeader>
					<CardContent class="space-y-2">
						{#if pointLimitEnabled}
							<div class="flex justify-between">
								<span class="text-muted-foreground text-sm">Target Points:</span>
								<Badge variant="secondary">
									{gameState.target_points}
								</Badge>
							</div>
						{/if}
						{#if timeLimitEnabled}
							<div class="flex justify-between">
								<span class="text-muted-foreground text-sm">Quiz Duration:</span>
								<Badge variant="secondary">
									{formatTime(gameState.game_duration_seconds)}
								</Badge>
							</div>
						{/if}
						<div class="flex justify-between">
							<span class="text-muted-foreground text-sm">Players:</span>
							<Badge variant="outline">
								{leaderboard().length}
							</Badge>
						</div>
						{#if currentPhase === 'Playing' && timeLimitEnabled}
							<div class="flex justify-between">
								<span class="text-muted-foreground text-sm">Time Remaining:</span>
								<Badge variant={displayTimer() <= 30 ? 'destructive' : 'default'}>
									{formatTime(displayTimer())}
								</Badge>
							</div>
						{/if}
					</CardContent>
				</Card>
			</div>

			<!-- Leaderboard -->
			<div class="lg:col-span-2">
				<Card>
					<CardHeader>
						<CardTitle>Leaderboard</CardTitle>
						<CardDescription>
							{#if leaderboard().length === 0}
								No players yet - waiting for first answers!
							{:else}
								Top players and their scores
							{/if}
						</CardDescription>
					</CardHeader>
					<CardContent>
						{#if leaderboard().length === 0}
							<div class="text-muted-foreground py-8 text-center">
								<div class="mb-4 text-4xl">🎯</div>
								<p>Start the quiz and players will appear here as they answer correctly!</p>
							</div>
						{:else}
							<div class="space-y-2">
								{#each leaderboard() as { player, points }, index (player)}
									<div class="bg-muted/50 flex items-center justify-between rounded-lg border p-3">
										<div class="flex items-center gap-3">
											<div
												class="flex h-8 w-8 items-center justify-center rounded-full text-sm font-bold
                                                {index === 0
													? 'bg-yellow-500 text-white'
													: index === 1
														? 'bg-gray-400 text-white'
														: index === 2
															? 'bg-amber-600 text-white'
															: 'bg-muted text-muted-foreground'}"
											>
												{index + 1}
											</div>
											<span class="text-lg font-medium">{player}</span>
											{#if index === 0 && points > 0}
												<Badge variant="outline" class="ml-2">👑 Leader</Badge>
											{/if}
										</div>
										<div class="flex items-center gap-2">
											<Badge
												variant={pointLimitEnabled && points >= gameState.target_points
													? 'default'
													: 'secondary'}
												class="px-3 py-1 text-lg"
											>
												{points}
												{points === 1 ? 'point' : 'points'}
											</Badge>
											{#if pointLimitEnabled && points >= gameState.target_points}
												<span class="text-2xl">🏆</span>
											{/if}
										</div>
									</div>
								{/each}
							</div>
						{/if}
					</CardContent>
				</Card>
			</div>
		</div>
	</div>
</div>
