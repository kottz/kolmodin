<script lang="ts">
	import type { DealNoDealPublicState } from '../types';

	interface Props {
		gameState: DealNoDealPublicState;
	}

	let { gameState }: Props = $props();

	function formatMoney(amount: number): string {
		return '$' + amount.toLocaleString();
	}

	// Extract game over data from phase
	const gameOverData = $derived(gameState.phase.type === 'GameOver' ? gameState.phase.data : null);

	const winnings = $derived(gameOverData?.winnings || 0);
	const originalCaseValue = $derived(gameOverData?.player_case_original_value || 0);
	const summary = $derived(gameOverData?.summary || '');

	// Determine if we made a good choice by comparing winnings to original case
	const wasGoodChoice = $derived(winnings > originalCaseValue);
	const difference = $derived(Math.abs(winnings - originalCaseValue));

	// Derived class for comparison result background
	const comparisonBgClass = $derived(wasGoodChoice ? 'bg-green-500/20' : 'bg-red-500/20');
</script>

<div class="h-full rounded-2xl bg-black/40 p-6 backdrop-blur-sm">
	<!-- Game Over Header -->
	<div class="mb-8 text-center">
		<div class="mb-2 text-2xl font-bold text-white drop-shadow-lg">🎉 GAME OVER! 🎉</div>
		<div class="text-lg text-white/80">{summary}</div>
	</div>

	<!-- Results Grid -->
	<div class="grid h-[calc(100%-12rem)] grid-cols-1 gap-6">
		<!-- What We Won -->
		<div
			class="flex flex-col items-center justify-center rounded-xl bg-yellow-500/20 p-6 text-center"
		>
			<div class="mb-2 text-xl font-semibold text-yellow-200">💰 You Won</div>
			<div class="text-6xl font-bold text-yellow-300 drop-shadow-lg">
				{formatMoney(winnings)}
			</div>
		</div>

		<!-- Original Case Value -->
		<div
			class="flex flex-col items-center justify-center rounded-xl bg-blue-500/20 p-6 text-center"
		>
			<div class="mb-2 text-xl font-semibold text-blue-200">🎁 Your Original Case</div>
			<div class="text-6xl font-bold text-blue-300 drop-shadow-lg">
				{formatMoney(originalCaseValue)}
			</div>
		</div>

		<!-- Comparison Result -->
		<div
			class="flex flex-col items-center justify-center rounded-xl p-6 text-center {comparisonBgClass}"
		>
			{#if wasGoodChoice}
				<div class="mb-2 text-xl font-semibold text-green-200">✅ Great Choice!</div>
				<div class="text-lg text-green-300">
					You won <span class="font-bold">{formatMoney(difference)}</span> more!
				</div>
			{:else if winnings === originalCaseValue}
				<div class="mb-2 text-xl font-semibold text-gray-200">⚖️ Perfect Match!</div>
				<div class="text-lg text-gray-300">You got exactly what was in your case!</div>
			{:else}
				<div class="mb-2 text-xl font-semibold text-red-200">💔 Could Have Been Better</div>
				<div class="text-lg text-red-300">
					Your case had <span class="font-bold">{formatMoney(difference)}</span> more
				</div>
			{/if}
		</div>
	</div>

	<!-- Final Message -->
	<div class="mt-6 text-center text-sm text-white/60">Thanks for playing Deal or No Deal!</div>
</div>
