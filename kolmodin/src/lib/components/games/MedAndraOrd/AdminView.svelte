<script lang="ts">
	import { medAndraOrdStore } from './store.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import Checkbox from '$lib/components/ui/checkbox.svelte';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card';
	import Badge from '$lib/components/ui/badge.svelte';

	const gameState = $derived(medAndraOrdStore.gameState);
	const leaderboard = $derived(medAndraOrdStore.leaderboard);
	const currentWord = $derived(medAndraOrdStore.currentWord);
	const winner = $derived(medAndraOrdStore.winner);
	const gameEndReason = $derived(medAndraOrdStore.gameEndReason);
	const displayWordTimer = $derived(medAndraOrdStore.displayWordTimer);
	const displayGameTimer = $derived(medAndraOrdStore.displayGameTimer);
	const currentPhase = $derived(gameState.phase.type);

	let targetPointsInput = $state(gameState.target_points.toString());
	let gameTimeLimitInput = $state(gameState.game_time_limit_minutes.toString());
	let pointLimitEnabled = $state(gameState.point_limit_enabled);
	let timeLimitEnabled = $state(gameState.time_limit_enabled);

	// Update inputs when game state changes
	$effect(() => {
		targetPointsInput = gameState.target_points.toString();
		gameTimeLimitInput = gameState.game_time_limit_minutes.toString();
		pointLimitEnabled = gameState.point_limit_enabled;
		timeLimitEnabled = gameState.time_limit_enabled;
	});

	function handleSetTargetPoints() {
		const points = parseInt(targetPointsInput);
		if (!isNaN(points) && points > 0) {
			medAndraOrdStore.actions.setTargetPoints(points);
		}
	}

	function handleSetGameTimeLimit() {
		const minutes = parseInt(gameTimeLimitInput);
		if (!isNaN(minutes) && minutes > 0) {
			medAndraOrdStore.actions.setGameTimeLimit(minutes);
		}
	}

	function handlePointLimitToggle() {
		medAndraOrdStore.actions.setPointLimitEnabled(pointLimitEnabled);
	}

	function handleTimeLimitToggle() {
		medAndraOrdStore.actions.setTimeLimitEnabled(timeLimitEnabled);
	}

	function getGameEndMessage(): string {
		if (gameEndReason === 'points') {
			return `${winner()} reached ${gameState.target_points} points!`;
		} else if (gameEndReason === 'time') {
			return `Time's up! ${winner() ? `${winner()} wins!` : 'No winner this round.'}`;
		}
		return `${winner()} wins!`;
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
						<h2 class="text-4xl font-bold">Med Andra Ord</h2>
						<p class="text-muted-foreground mt-2 text-lg">Ready to start the word guessing game!</p>
						<p class="text-muted-foreground mt-1 text-sm">
							Configure settings and click Start Game
						</p>
					{:else if currentPhase === 'Playing'}
						<h2 class="mb-4 text-4xl font-bold">Current Word</h2>
						<div class="text-primary mb-4 font-mono text-6xl font-bold">
							{currentWord() || 'Loading...'}
						</div>
						<div class="grid grid-cols-1 gap-4 md:grid-cols-2">
							<div class="text-muted-foreground text-xl">
								Word Time: {formatTime(displayWordTimer())}
							</div>
							{#if timeLimitEnabled}
								<div class="text-muted-foreground text-xl">
									Game Time: {formatTime(displayGameTimer())}
								</div>
							{/if}
						</div>
						<p class="text-muted-foreground mt-2 text-sm">
							Describe this word to your Twitch chat! First correct guess wins a point.
						</p>
					{:else if currentPhase === 'GameOver'}
						<h2 class="text-4xl font-bold text-green-600">Game Over!</h2>
						<div class="mt-4 text-3xl font-bold">
							🎉 {getGameEndMessage()} 🎉
						</div>
						<p class="text-muted-foreground mt-2 text-lg">
							{#if gameEndReason === 'points'}
								Target points reached!
							{:else if gameEndReason === 'time'}
								Time limit reached!
							{:else}
								Game completed!
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
						<CardDescription>Control the game flow and settings</CardDescription>
					</CardHeader>
					<CardContent class="space-y-4">
						{#if currentPhase === 'Setup'}
							<!-- Point Limit Settings -->
							<div class="space-y-3">
								<div class="flex items-center space-x-2">
									<Checkbox
										id="point-limit-enabled"
										bind:checked={pointLimitEnabled}
										onCheckedChange={handlePointLimitToggle}
									/>
									<span>Target Points</span>
								</div>
								<div class="space-y-2">
									<div class="flex gap-2">
										<Input
											id="target-points"
											type="number"
											min="1"
											max="50"
											bind:value={targetPointsInput}
											placeholder="10"
											class="flex-1"
											disabled={!pointLimitEnabled}
										/>
										<Button
											onclick={handleSetTargetPoints}
											variant="outline"
											size="sm"
											disabled={!pointLimitEnabled}
										>
											Set
										</Button>
									</div>
								</div>
							</div>

							<!-- Time Limit Settings -->
							<div class="space-y-3">
								<div class="flex items-center space-x-2">
									<Checkbox
										id="time-limit-enabled"
										bind:checked={timeLimitEnabled}
										onCheckedChange={handleTimeLimitToggle}
									/>
									<span>Time Limit</span>
								</div>
								<div class="space-y-2">
									<div class="flex gap-2">
										<Input
											id="game-time-limit"
											type="number"
											min="1"
											max="60"
											bind:value={gameTimeLimitInput}
											placeholder="5"
											class="flex-1"
											disabled={!timeLimitEnabled}
										/>
										<Button
											onclick={handleSetGameTimeLimit}
											variant="outline"
											size="sm"
											disabled={!timeLimitEnabled}
										>
											Set
										</Button>
									</div>
								</div>
							</div>
							<Button onclick={medAndraOrdStore.actions.startGame} class="w-full" size="lg">
								Start Game
							</Button>
						{:else if currentPhase === 'Playing'}
							<div class="flex flex-col gap-2">
								<Button
									onclick={medAndraOrdStore.actions.passWord}
									variant="outline"
									class="w-full"
								>
									Skip Word
								</Button>
								<Button
									onclick={medAndraOrdStore.actions.resetGame}
									variant="destructive"
									class="w-full"
								>
									End Game
								</Button>
							</div>
						{:else if currentPhase === 'GameOver'}
							<Button onclick={medAndraOrdStore.actions.resetGame} class="w-full" size="lg">
								New Game
							</Button>
						{/if}
					</CardContent>
				</Card>

				<!-- Game Info -->
				<Card>
					<CardHeader>
						<CardTitle>Game Info</CardTitle>
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
								<span class="text-muted-foreground text-sm">Game Time Limit:</span>
								<Badge variant="secondary">
									{gameState.game_time_limit_minutes} min
								</Badge>
							</div>
						{/if}
						<div class="flex justify-between">
							<span class="text-muted-foreground text-sm">Players:</span>
							<Badge variant="outline">
								{leaderboard().length}
							</Badge>
						</div>
						{#if currentPhase === 'Playing'}
							<div class="flex justify-between">
								<span class="text-muted-foreground text-sm">Word Time:</span>
								<Badge variant={displayWordTimer() <= 10 ? 'destructive' : 'default'}>
									{formatTime(displayWordTimer())}
								</Badge>
							</div>
							{#if timeLimitEnabled}
								<div class="flex justify-between">
									<span class="text-muted-foreground text-sm">Game Time:</span>
									<Badge variant={displayGameTimer() <= 60 ? 'destructive' : 'default'}>
										{formatTime(displayGameTimer())}
									</Badge>
								</div>
							{/if}
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
								No players yet - waiting for first guesses!
							{:else}
								Top players and their scores
							{/if}
						</CardDescription>
					</CardHeader>
					<CardContent>
						{#if leaderboard().length === 0}
							<div class="text-muted-foreground py-8 text-center">
								<div class="mb-4 text-4xl">🎯</div>
								<p>Start the game and players will appear here as they guess correctly!</p>
							</div>
						{:else}
							<div class="space-y-2">
								{#each leaderboard() as { player, points }, index}
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
