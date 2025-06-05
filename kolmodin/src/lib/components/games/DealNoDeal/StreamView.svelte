<script lang="ts">
	import { fade, fly, scale, blur } from 'svelte/transition';
	import { quintOut, elasticOut } from 'svelte/easing';
	import type { DealNoDealPublicState } from './types';
	import type { StreamDisplayConfig } from '$lib/types/stream.types';

	interface Props {
		gameState: DealNoDealPublicState;
		displayConfig: StreamDisplayConfig;
	}

	let { gameState, displayConfig }: Props = $props();

	function getPhaseTitle(phase: string): string {
		switch (phase) {
			case 'Setup':
				return 'Setting Up The Game';
			case 'PlayerCaseSelectionVoting':
				return 'Choose Your Case!';
			case 'RoundCaseOpeningVoting':
				return 'Opening Cases...';
			case 'BankerOfferCalculation':
				return 'Banker is Calculating...';
			case 'DealOrNoDealVoting':
				return 'Deal or No Deal?';
			case 'SwitchOrKeepVoting':
				return 'Switch or Keep?';
			case 'GameOver':
				return 'Final Reveal!';
			default:
				return 'Deal or No Deal';
		}
	}

	function getPhaseEmoji(phase: string): string {
		switch (phase) {
			case 'Setup':
				return '⚙️';
			case 'PlayerCaseSelectionVoting':
				return '🎯';
			case 'RoundCaseOpeningVoting':
				return '📦';
			case 'BankerOfferCalculation':
				return '🏦';
			case 'DealOrNoDealVoting':
				return '🤝';
			case 'SwitchOrKeepVoting':
				return '🔄';
			case 'GameOver':
				return '💰';
			default:
				return '🎮';
		}
	}

	function formatMoney(amount: number): string {
		return '$' + amount.toLocaleString();
	}

	function getMoneyColor(amount: number): string {
		if (amount >= 100000) return 'text-red-400'; // High amounts in red
		if (amount >= 10000) return 'text-yellow-400'; // Medium amounts in yellow
		return 'text-blue-400'; // Low amounts in blue
	}

	function getVoteColor(voteType: 'DEAL' | 'NO DEAL' | 'SWITCH' | 'KEEP'): string {
		switch (voteType) {
			case 'DEAL':
				return 'from-green-500 to-emerald-600';
			case 'NO DEAL':
				return 'from-red-500 to-rose-600';
			case 'SWITCH':
				return 'from-blue-500 to-cyan-600';
			case 'KEEP':
				return 'from-purple-500 to-violet-600';
		}
	}

	// Derived states
	const isPlaying = $derived(gameState.phase.type !== 'Setup');
	const hasPlayerCase = $derived(gameState.playerChosenCaseIndex !== null);
	const bankerOffer = $derived(
		gameState.phase.type === 'DealOrNoDealVoting' ? gameState.phase.data?.offer : null
	);
	const isGameOver = $derived(gameState.phase.type === 'GameOver');
	const totalCases = $derived(gameState.briefcases?.length || 0);
	const openedCases = $derived(gameState.briefcases?.filter((c) => c.isOpened).length || 0);

	// Group money values into columns like the admin view
	const moneyBoardColumns = $derived(() => {
		if (!gameState.remainingMoneyValues || gameState.remainingMoneyValues.length === 0) {
			return { left: [], right: [] };
		}
		const sortedValues = [...gameState.remainingMoneyValues].sort((a, b) => a - b);
		const midpoint = Math.ceil(sortedValues.length / 2);
		return {
			left: sortedValues.slice(0, midpoint),
			right: sortedValues.slice(midpoint)
		};
	});
</script>

<div
	class="stream-view-container relative h-screen overflow-hidden bg-gradient-to-br from-slate-900 via-blue-900 to-indigo-900"
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
			{#each Array(20) as _, i (i)}
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
		<!-- Header -->
		<header class="px-6 py-6 text-center">
			<div
				class="mb-4 text-6xl"
				class:animate-bounce={gameState.phase.type === 'BankerOfferCalculation'}
				transition:scale={{ duration: 500, easing: quintOut }}
			>
				{getPhaseEmoji(gameState.phase.type)}
			</div>
			<h1
				class="mb-2 text-5xl font-bold tracking-wide text-white"
				transition:fade={{ duration: 300 }}
			>
				Deal or No Deal
			</h1>
			<h2 class="text-2xl font-medium text-white/90" transition:fly={{ y: 20, duration: 400 }}>
				{getPhaseTitle(gameState.phase.type)}
			</h2>
		</header>

		<!-- Main Content Area -->
		<div class="grid flex-1 grid-cols-1 gap-6 px-6 pb-6 lg:grid-cols-3">
			<!-- Left Column: Money Board -->
			{#if isPlaying && gameState.remainingMoneyValues && gameState.remainingMoneyValues.length > 0}
				<div
					class="rounded-2xl bg-black/30 p-6 backdrop-blur-sm"
					transition:fly={{ x: -50, duration: 500 }}
				>
					<h3 class="mb-4 text-center text-xl font-bold text-white">💰 Money Board 💰</h3>
					<div class="grid grid-cols-2 gap-2 text-lg">
						<!-- Left Column -->
						<div class="space-y-2">
							{#each moneyBoardColumns().left as amount (amount)}
								<div
									class="rounded-lg px-3 py-2 text-center font-bold transition-all duration-300 {gameState.remainingMoneyValues.includes(
										amount
									)
										? 'bg-white/20 text-white'
										: 'bg-red-900/50 text-gray-400 line-through opacity-50'}"
								>
									{formatMoney(amount)}
								</div>
							{/each}
						</div>
						<!-- Right Column -->
						<div class="space-y-2">
							{#each moneyBoardColumns().right as amount (amount)}
								<div
									class="rounded-lg px-3 py-2 text-center font-bold transition-all duration-300 {gameState.remainingMoneyValues.includes(
										amount
									)
										? 'bg-white/20 text-white'
										: 'bg-red-900/50 text-gray-400 line-through opacity-50'}"
								>
									{formatMoney(amount)}
								</div>
							{/each}
						</div>
					</div>
				</div>
			{/if}

			<!-- Center Column: Main Action -->
			<div
				class="flex flex-col justify-center rounded-2xl bg-black/30 p-6 backdrop-blur-sm"
				class:lg:col-span-2={!isPlaying}
				transition:fly={{ y: 30, duration: 500 }}
			>
				{#if gameState.phase.type === 'Setup'}
					<!-- Setup Phase -->
					<div class="space-y-6 text-center">
						<div class="animate-spin text-8xl">⚙️</div>
						<h3 class="text-3xl font-bold text-white">Setting Up Game</h3>
						<p class="text-lg text-white/80">Get ready for the ultimate game of risk and reward!</p>
						<div class="space-y-2 text-white/60">
							<p>📦 {totalCases} briefcases with hidden amounts</p>
							<p>🏦 The banker will make offers</p>
							<p>🎯 Will you take the deal or risk it all?</p>
						</div>
					</div>
				{:else if gameState.phase.type === 'PlayerCaseSelectionVoting'}
					<!-- Case Selection Phase -->
					<div class="space-y-6 text-center">
						<div class="animate-pulse text-7xl">🎯</div>
						<h3 class="text-3xl font-bold text-white">Choose Your Lucky Case!</h3>
						<p class="text-lg text-white/90">
							One of these briefcases could contain the big money...
						</p>
						{#if hasPlayerCase}
							<div
								class="rounded-xl bg-yellow-500/20 p-4"
								transition:scale={{ duration: 500, easing: elasticOut }}
							>
								<p class="text-xl font-bold text-yellow-300">
									Case #{(gameState.playerChosenCaseIndex || 0) + 1} Selected!
								</p>
								<p class="text-white/80">Your fate is sealed in this case...</p>
							</div>
						{/if}
					</div>
				{:else if gameState.phase.type === 'RoundCaseOpeningVoting'}
					<!-- Case Opening Phase -->
					<div class="space-y-6 text-center">
						<div class="animate-bounce text-7xl">📦</div>
						<h3 class="text-3xl font-bold text-white">Opening Cases...</h3>
						{#if gameState.phase.data}
							<div class="text-xl text-white/90">
								{gameState.phase.data.opened_so_far_for_round || 0} / {gameState.phase.data
									.total_to_open_for_round || 0}
								cases opened this round
							</div>
							<div class="rounded-xl bg-white/10 p-4">
								<div class="h-4 overflow-hidden rounded-full bg-gray-700">
									<div
										class="h-full bg-gradient-to-r from-blue-500 to-green-500 transition-all duration-500"
										style="width: {((gameState.phase.data.opened_so_far_for_round || 0) /
											(gameState.phase.data.total_to_open_for_round || 1)) *
											100}%"
									></div>
								</div>
							</div>
						{/if}
						<p class="text-white/80">Eliminating amounts from the board...</p>
					</div>
				{:else if gameState.phase.type === 'BankerOfferCalculation'}
					<!-- Banker Calculating -->
					<div class="space-y-6 text-center">
						<div class="animate-bounce text-7xl">🏦</div>
						<h3 class="text-3xl font-bold text-white">Banker is Calculating...</h3>
						<div class="flex justify-center">
							<div class="rounded-xl bg-white/10 p-6">
								<div class="mb-4 animate-spin text-4xl">💭</div>
								<p class="text-white/90">The banker is reviewing the remaining amounts</p>
								<p class="text-white/70">An offer is coming...</p>
							</div>
						</div>
					</div>
				{:else if gameState.phase.type === 'DealOrNoDealVoting'}
					<!-- Deal or No Deal Decision -->
					<div class="space-y-6 text-center">
						<div class="animate-pulse text-7xl">🤝</div>
						<h3 class="text-3xl font-bold text-white">Deal or No Deal?</h3>
						{#if bankerOffer}
							<div
								class="to-gold-500/20 rounded-2xl bg-gradient-to-r from-yellow-500/20 p-6"
								transition:scale={{ duration: 500, easing: elasticOut }}
							>
								<p class="mb-2 text-lg text-white/90">Banker's Offer:</p>
								<p class="text-5xl font-bold text-yellow-300">{formatMoney(bankerOffer)}</p>
							</div>
						{/if}

						{#if gameState.voteCounts}
							<div class="grid grid-cols-2 gap-4">
								<!-- Deal Votes -->
								<div class="rounded-xl bg-gradient-to-br from-green-500/20 to-emerald-600/20 p-4">
									<h4 class="mb-2 text-xl font-bold text-green-300">✅ DEAL</h4>
									<p class="text-2xl font-bold text-white">{gameState.voteCounts.DEAL || 0}</p>
									<p class="text-sm text-green-200">Take the money!</p>
								</div>

								<!-- No Deal Votes -->
								<div class="rounded-xl bg-gradient-to-br from-red-500/20 to-rose-600/20 p-4">
									<h4 class="mb-2 text-xl font-bold text-red-300">❌ NO DEAL</h4>
									<p class="text-2xl font-bold text-white">
										{gameState.voteCounts['NO DEAL'] || 0}
									</p>
									<p class="text-sm text-red-200">Keep playing!</p>
								</div>
							</div>
						{/if}
					</div>
				{:else if gameState.phase.type === 'SwitchOrKeepVoting'}
					<!-- Switch or Keep Decision -->
					<div class="space-y-6 text-center">
						<div class="animate-pulse text-7xl">🔄</div>
						<h3 class="text-3xl font-bold text-white">Final Decision!</h3>
						<div class="rounded-xl bg-white/10 p-6">
							<p class="mb-4 text-lg text-white/90">Your case vs. the final case:</p>
							<div class="grid grid-cols-2 gap-4">
								<div class="rounded-lg bg-blue-500/20 p-4">
									<p class="font-bold text-blue-300">Your Case</p>
									<p class="text-2xl">#{(gameState.playerChosenCaseIndex || 0) + 1}</p>
								</div>
								<div class="rounded-lg bg-purple-500/20 p-4">
									<p class="font-bold text-purple-300">Final Case</p>
									<p class="text-2xl">
										#{gameState.phase.data?.final_case_index
											? gameState.phase.data.final_case_index + 1
											: '?'}
									</p>
								</div>
							</div>
						</div>

						{#if gameState.voteCounts}
							<div class="grid grid-cols-2 gap-4">
								<!-- Switch Votes -->
								<div class="rounded-xl bg-gradient-to-br from-blue-500/20 to-cyan-600/20 p-4">
									<h4 class="mb-2 text-xl font-bold text-blue-300">🔄 SWITCH</h4>
									<p class="text-2xl font-bold text-white">{gameState.voteCounts.SWITCH || 0}</p>
								</div>

								<!-- Keep Votes -->
								<div class="rounded-xl bg-gradient-to-br from-purple-500/20 to-violet-600/20 p-4">
									<h4 class="mb-2 text-xl font-bold text-purple-300">🛡️ KEEP</h4>
									<p class="text-2xl font-bold text-white">{gameState.voteCounts.KEEP || 0}</p>
								</div>
							</div>
						{/if}
					</div>
				{:else if gameState.phase.type === 'GameOver'}
					<!-- Game Over -->
					<div class="space-y-6 text-center">
						<div class="animate-bounce text-8xl">💰</div>
						<h3 class="text-4xl font-bold text-white">Final Reveal!</h3>
						{#if gameState.phase.data}
							<div
								class="to-gold-500/20 rounded-2xl bg-gradient-to-r from-yellow-500/20 p-8"
								transition:scale={{ duration: 800, easing: elasticOut }}
							>
								<p class="mb-4 text-2xl text-white/90">Final Winnings:</p>
								<p class="mb-4 text-6xl font-bold text-yellow-300">
									{formatMoney(gameState.phase.data.winnings || 0)}
								</p>
								{#if gameState.phase.data.player_case_original_value}
									<p class="text-lg text-white/80">
										Your case actually contained: {formatMoney(
											gameState.phase.data.player_case_original_value
										)}
									</p>
								{/if}
							</div>
						{/if}
						<p class="text-xl text-white/90">
							{gameState.phase.data?.winnings && gameState.phase.data.winnings > 0
								? '🎉 Congratulations! 🎉'
								: 'Better luck next time!'}
						</p>
					</div>
				{/if}
			</div>

			<!-- Right Column: Game Status -->
			{#if isPlaying && gameState.phase.type !== 'Setup'}
				<div
					class="rounded-2xl bg-black/30 p-6 backdrop-blur-sm"
					transition:fly={{ x: 50, duration: 500 }}
				>
					<h3 class="mb-4 text-center text-xl font-bold text-white">📊 Game Status</h3>
					<div class="space-y-4">
						{#if hasPlayerCase}
							<div class="rounded-lg bg-yellow-500/20 p-3">
								<p class="text-sm text-white/80">Your Case:</p>
								<p class="text-2xl font-bold text-yellow-300">
									#{(gameState.playerChosenCaseIndex || 0) + 1}
								</p>
							</div>
						{/if}

						<div class="rounded-lg bg-white/10 p-3">
							<p class="text-sm text-white/80">Cases Opened:</p>
							<p class="text-xl font-bold text-white">
								{openedCases} / {totalCases}
							</p>
						</div>

						{#if gameState.remainingMoneyValues}
							<div class="rounded-lg bg-white/10 p-3">
								<p class="text-sm text-white/80">Amounts Left:</p>
								<p class="text-xl font-bold text-white">
									{gameState.remainingMoneyValues.length}
								</p>
							</div>
						{/if}

						{#if bankerOffer}
							<div class="rounded-lg bg-green-500/20 p-3">
								<p class="text-sm text-white/80">Current Offer:</p>
								<p class="text-lg font-bold text-green-300">
									{formatMoney(bankerOffer)}
								</p>
							</div>
						{/if}
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
