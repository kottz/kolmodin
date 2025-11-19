<script lang="ts">
	import type { DealNoDealPublicState } from './types';
	import type { StreamDisplayConfig } from '$lib/types/stream.types';
	import DealVotingView from './components/DealVotingView.svelte';
	import SwitchKeepVotingView from './components/SwitchKeepVotingView.svelte';
	import GameOverView from './components/GameOverView.svelte';

	interface Props {
		gameState: DealNoDealPublicState;
		displayConfig: StreamDisplayConfig;
	}

	let { gameState, displayConfig }: Props = $props();

	// displayConfig can be used for future customization - currently unused
	void displayConfig;

	function formatMoney(amount: number): string {
		return '$' + amount.toLocaleString();
	}

	// Derived states
	const isPlaying = $derived(gameState.phase.type !== 'Setup');
	const hasPlayerCase = $derived(gameState.playerChosenCaseIndex !== null);
	const bankerOffer = $derived(
		gameState.phase.type === 'DealOrNoDealVoting' ? gameState.phase.data?.offer : null
	);
	const isGameOver = $derived(gameState.phase.type === 'GameOver');

	// Group money values into columns like the admin view - use all money values
	const moneyBoardColumns = $derived(() => {
		if (!gameState.allMoneyValues || gameState.allMoneyValues.length === 0) {
			return { left: [], right: [] };
		}
		const sortedValues = [...gameState.allMoneyValues].sort((a, b) => a - b);
		const midpoint = Math.ceil(sortedValues.length / 2);
		return {
			left: sortedValues.slice(0, midpoint),
			right: sortedValues.slice(midpoint)
		};
	});
</script>

<div
	class="stream-view-container relative h-screen overflow-hidden bg-linear-to-br from-slate-900 via-blue-900 to-indigo-900"
>
	<!-- Animated Background Elements -->
	<div class="absolute inset-0 opacity-20">
		<div
			class="bg-gold-500 absolute top-1/4 left-1/4 h-64 w-64 animate-pulse rounded-full blur-3xl"
		></div>
		<div
			class="absolute right-1/4 bottom-1/3 h-96 w-96 animate-pulse rounded-full bg-blue-500 blur-3xl delay-1000"
		></div>
		<div
			class="absolute top-2/3 left-1/2 h-48 w-48 animate-pulse rounded-full bg-green-500 blur-2xl delay-2000"
		></div>
	</div>

	<!-- Money Rain Animation for Big Reveals -->
	{#if isGameOver}
		<div class="pointer-events-none absolute inset-0">
			{#each Array.from({ length: 20 }, (_, i) => i) as coinIndex (coinIndex)}
				<div
					class="absolute animate-bounce text-4xl"
					style="left: {Math.random() * 100}%; animation-delay: {Math.random() *
						2}s; animation-duration: {2 + Math.random() * 2}s;"
				>
					💰
				</div>
			{/each}
		</div>
	{/if}

	<!-- Main Content -->
	<div class="relative z-10 flex h-full flex-col">
		<!-- Top Information Bar -->
		<div class="bg-black/40 p-4 backdrop-blur-sm">
			<div class="flex items-center justify-between">
				<!-- Phase Information -->
				<div class="text-white">
					{#if gameState.phase.type === 'Setup'}
						<div class="flex items-center gap-3">
							<div class="animate-spin text-2xl">⚙️</div>
							<span class="text-xl font-bold">Setting Up Game</span>
						</div>
					{:else if gameState.phase.type === 'PlayerCaseSelectionVoting'}
						<div class="flex items-center gap-3">
							<div class="animate-pulse text-2xl">🎯</div>
							<span class="text-xl font-bold">Choose Your Case!</span>
						</div>
					{:else if gameState.phase.type === 'RoundCaseOpeningVoting'}
						<div class="flex items-center gap-3">
							<div class="animate-bounce text-2xl">📦</div>
							<span class="text-xl font-bold">Opening Cases...</span>
							{#if gameState.phase.data}
								<span class="text-white/80">
									({gameState.phase.data.opened_so_far_for_round || 0} / {gameState.phase.data
										.total_to_open_for_round || 0})
								</span>
							{/if}
						</div>
					{:else if gameState.phase.type === 'BankerOfferCalculation'}
						<div class="flex items-center gap-3">
							<div class="animate-bounce text-2xl">🏦</div>
							<span class="text-xl font-bold">Banker is Calculating...</span>
						</div>
					{:else if gameState.phase.type === 'DealOrNoDealVoting'}
						<div class="flex items-center gap-3">
							<div class="animate-pulse text-2xl">🤝</div>
							<span class="text-xl font-bold">Deal or No Deal?</span>
						</div>
					{:else if gameState.phase.type === 'SwitchOrKeepVoting'}
						<div class="flex items-center gap-3">
							<div class="animate-pulse text-2xl">🔄</div>
							<span class="text-xl font-bold">Switch or Keep?</span>
						</div>
					{:else if gameState.phase.type === 'GameOver'}
						<div class="flex items-center gap-3">
							<div class="animate-bounce text-2xl">💰</div>
							<span class="text-xl font-bold">Final Reveal!</span>
							{#if gameState.phase.data}
								<span class="font-bold text-yellow-300">
									{formatMoney(gameState.phase.data.winnings || 0)}
								</span>
							{/if}
						</div>
					{/if}
				</div>

				<!-- Voting Instructions -->
				<div class="text-center text-white">
					{#if gameState.phase.type === 'PlayerCaseSelectionVoting'}
						<div class="rounded-lg bg-white/10 p-3">
							<div class="text-lg">Type a case number in chat to vote</div>
						</div>
					{:else if gameState.phase.type === 'RoundCaseOpeningVoting'}
						<div class="rounded-lg bg-white/10 p-3">
							<div class="text-lg">Type a case number in chat to vote</div>
						</div>
					{:else if gameState.phase.type === 'DealOrNoDealVoting'}
						<div class="rounded-lg bg-white/10 p-3">
							<div class="text-lg">
								Type <span class="font-bold">deal</span> or <span class="font-bold">no deal</span> in
								chat
							</div>
						</div>
					{:else if gameState.phase.type === 'SwitchOrKeepVoting'}
						<div class="rounded-lg bg-white/10 p-3">
							<div class="text-lg">
								Type <span class="font-bold">switch</span> or <span class="font-bold">keep</span> in
								chat
							</div>
						</div>
					{:else if gameState.phase.type === 'BankerOfferCalculation'}
						<div class="rounded-lg bg-yellow-500/20 p-3">
							<div class="text-lg text-yellow-200">Banker is calculating offer...</div>
						</div>
					{:else if gameState.phase.type === 'GameOver'}
						<div class="rounded-lg bg-green-500/20 p-3">
							<div class="text-lg text-green-200">Game Over!</div>
						</div>
					{:else if hasPlayerCase}
						<div class="rounded-lg bg-blue-500/20 p-3">
							<span class="text-sm text-blue-200">Your Case:</span>
							<div class="text-xl font-bold text-blue-300">
								#{(gameState.playerChosenCaseIndex || 0) + 1}
							</div>
						</div>
					{/if}
				</div>

				<!-- Vote Counts for Decision Phases -->
				{#if gameState.phase.type === 'DealOrNoDealVoting' && gameState.voteCounts}
					<div class="flex gap-4">
						<div class="rounded-lg bg-green-500/20 p-2 text-center">
							<div class="text-xs text-green-200">DEAL</div>
							<div class="text-lg font-bold text-white">{gameState.voteCounts.DEAL || 0}</div>
						</div>
						<div class="rounded-lg bg-red-500/20 p-2 text-center">
							<div class="text-xs text-red-200">NO DEAL</div>
							<div class="text-lg font-bold text-white">{gameState.voteCounts['NO DEAL'] || 0}</div>
						</div>
					</div>
				{:else if gameState.phase.type === 'SwitchOrKeepVoting' && gameState.voteCounts}
					<div class="flex gap-4">
						<div class="rounded-lg bg-blue-500/20 p-2 text-center">
							<div class="text-xs text-blue-200">SWITCH</div>
							<div class="text-lg font-bold text-white">{gameState.voteCounts.SWITCH || 0}</div>
						</div>
						<div class="rounded-lg bg-purple-500/20 p-2 text-center">
							<div class="text-xs text-purple-200">KEEP</div>
							<div class="text-lg font-bold text-white">{gameState.voteCounts.KEEP || 0}</div>
						</div>
					</div>
				{/if}
			</div>
		</div>

		<!-- Main Game Area: Money Board Left, Briefcases Right -->
		{#if isPlaying}
			<div class="flex flex-1 gap-6 p-6">
				<!-- Left: Money Board -->
				<div class="w-1/3">
					<div class="h-full rounded-2xl bg-black/30 p-6 backdrop-blur-sm">
						<h3 class="mb-4 text-center text-xl font-bold text-white">💰 Money Board 💰</h3>
						{#if moneyBoardColumns().left.length > 0}
							<div class="grid grid-cols-2 gap-x-3 gap-y-1.5 text-lg">
								<div class="flex flex-col space-y-1.5">
									{#each moneyBoardColumns().left as moneyValue (moneyValue)}
										{@const isActive = gameState.remainingMoneyValues?.includes(moneyValue)}
										<div
											class="rounded px-2 py-1.5 text-center font-medium shadow-sm
											{isActive
												? moneyValue >= 100000
													? 'bg-red-500 text-white'
													: 'bg-blue-500 text-white'
												: 'bg-gray-700 text-gray-400 line-through opacity-60'}"
										>
											{formatMoney(moneyValue)}
										</div>
									{/each}
								</div>
								<div class="flex flex-col space-y-1.5">
									{#each moneyBoardColumns().right as moneyValue (moneyValue)}
										{@const isActive = gameState.remainingMoneyValues?.includes(moneyValue)}
										<div
											class="rounded px-2 py-1.5 text-center font-medium shadow-sm
											{isActive
												? moneyValue >= 100000
													? 'bg-red-500 text-white'
													: 'bg-blue-500 text-white'
												: 'bg-gray-700 text-gray-400 line-through opacity-60'}"
										>
											{formatMoney(moneyValue)}
										</div>
									{/each}
								</div>
							</div>
						{:else}
							<p class="text-white/60">Money values not yet available.</p>
						{/if}
					</div>
				</div>

				<!-- Right: Briefcases, Deal Voting View, Switch/Keep Voting View, or Game Over View -->
				<div class="w-2/3">
					{#if gameState.phase.type === 'GameOver'}
						<!-- Show Game Over View when game is complete -->
						<GameOverView {gameState} />
					{:else if gameState.phase.type === 'DealOrNoDealVoting'}
						<!-- Show Deal Voting View during voting phase -->
						<DealVotingView {gameState} />
					{:else if gameState.phase.type === 'SwitchOrKeepVoting'}
						<!-- Show Switch/Keep Voting View during final decision phase -->
						<SwitchKeepVotingView {gameState} />
					{:else}
						<!-- Show Briefcases during other phases -->
						<div class="h-full rounded-2xl bg-black/30 p-6 backdrop-blur-sm">
							<h3 class="mb-4 text-center text-xl font-bold text-white">💼 Briefcases 💼</h3>
							{#if gameState.briefcases && gameState.briefcases.length > 0}
								<div class="grid grid-cols-4 gap-2 sm:grid-cols-5 md:grid-cols-6 lg:grid-cols-6">
									{#each gameState.briefcases as briefcase (briefcase.index)}
										{@const caseIndex = briefcase.index}
										{@const isOpened = briefcase.isOpened}
										{@const caseValue = briefcase.value}
										{@const isPlayerCase = gameState.playerChosenCaseIndex === caseIndex}
										{@const votersForThisCase = gameState.caseVotes?.[caseIndex] || []}

										<div
											class="flex aspect-square flex-col items-center justify-start rounded border p-1 text-center text-xs shadow-sm sm:p-2 sm:text-sm
											{isOpened
												? 'border-gray-600 bg-gray-900/50 text-gray-400 line-through opacity-70'
												: isPlayerCase
													? 'border-yellow-400 bg-yellow-500/20 text-yellow-100 ring-2 ring-yellow-400'
													: 'border-white/30 bg-white/10 text-white hover:bg-white/20'}"
										>
											<div>
												<div class="shrink-0">
													<span class="font-bold">#{caseIndex + 1}</span>
													{#if isPlayerCase && !isOpened}
														<span class="block text-[0.6rem] text-yellow-300 sm:text-xs"
															>(Your Pick)</span
														>
													{/if}
												</div>

												{#if isOpened && caseValue !== undefined}
													<span class="mt-0.5 block text-[0.6rem] sm:text-xs"
														>{formatMoney(caseValue)}</span
													>
												{/if}
											</div>

											{#if !isOpened && votersForThisCase && votersForThisCase.length > 0}
												{@const maxVisibleVotes = 6}
												{@const visibleVotes = votersForThisCase.slice(0, maxVisibleVotes)}
												{@const remainingCount = Math.max(
													0,
													votersForThisCase.length - maxVisibleVotes
												)}
												<div class="mt-auto w-full pt-0.5">
													<div class="flex flex-wrap justify-center gap-0.5">
														{#each visibleVotes as voter (voter)}
															<div
																class="rounded bg-white/20 px-1 py-0.5 text-lg font-medium text-white"
															>
																{voter}
															</div>
														{/each}
														{#if remainingCount > 0}
															<div
																class="rounded bg-white/30 px-1 py-0.5 text-lg font-medium text-white"
															>
																+{remainingCount}
															</div>
														{/if}
													</div>
												</div>
											{/if}
										</div>
									{/each}
								</div>
							{:else}
								<p class="text-white/60">Briefcases not yet initialized.</p>
							{/if}
						</div>
					{/if}
				</div>
			</div>
		{:else}
			<!-- Setup phase centered content -->
			<div class="flex flex-1 items-center justify-center p-6">
				<div class="space-y-6 text-center">
					<div class="animate-spin text-8xl">⚙️</div>
					<h3 class="text-3xl font-bold text-white">Setting Up Game</h3>
					<p class="text-lg text-white/80">Get ready for the ultimate game of risk and reward!</p>
					<div class="space-y-2 text-white/60">
						<p>📦 Briefcases with hidden amounts</p>
						<p>🏦 The banker will make offers</p>
						<p>🎯 Will you take the deal or risk it all?</p>
					</div>
				</div>
			</div>
		{/if}
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

	/* Custom gold color */
	.bg-gold-500 {
		background-color: #f59e0b;
	}

	.text-gold-500 {
		color: #f59e0b;
	}

	.from-gold-500 {
		--tw-gradient-from: #f59e0b;
	}

	.to-gold-500 {
		--tw-gradient-to: #f59e0b;
	}

	/* Animation for money rain */
	@keyframes money-fall {
		0% {
			transform: translateY(-100vh) rotate(0deg);
			opacity: 1;
		}
		100% {
			transform: translateY(100vh) rotate(360deg);
			opacity: 0;
		}
	}
</style>
