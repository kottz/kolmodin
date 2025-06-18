<script lang="ts">
	import type { DealNoDealPublicState } from '../types';

	interface Props {
		gameState: DealNoDealPublicState;
	}

	let { gameState }: Props = $props();

	function formatMoney(amount: number): string {
		return '$' + amount.toLocaleString();
	}

	// Extract banker offer from voting phase
	const bankerOffer = $derived(
		gameState.phase.type === 'DealOrNoDealVoting' ? gameState.phase.data?.offer : 0
	);

	// Get vote counts and voter names from gameState
	const dealCount = $derived(gameState.dealVotes?.deal?.length || 0);
	const noDealCount = $derived(gameState.dealVotes?.noDeal?.length || 0);
	const displayDealVoters = $derived(() => gameState.dealVotes?.deal || []);
	const displayNoDealVoters = $derived(() => gameState.dealVotes?.noDeal || []);

	const maxVisibleVoters = 8;
</script>

<div class="h-full rounded-2xl bg-black/30 p-6 backdrop-blur-sm">
	<!-- Top Title Bar with Banker's Offer -->
	<div class="mb-6 text-center">
		<div class="mb-2 text-lg text-white/80">🏦 Banker's Offer</div>
		<div class="text-4xl font-bold text-yellow-300 drop-shadow-lg">
			{formatMoney(bankerOffer || 0)}
		</div>
	</div>

	<!-- Voting Columns -->
	<div class="grid h-[calc(100%-8rem)] grid-cols-2 gap-6">
		<!-- Deal Column -->
		<div class="flex flex-col rounded-xl bg-green-500/10 p-4">
			<div class="mb-4 text-center">
				<div class="text-2xl font-bold text-green-300">💚 Deal?</div>
				<div class="text-lg text-green-200">({dealCount} votes)</div>
			</div>
			<div class="flex-1 overflow-y-auto">
				<div class="flex flex-wrap gap-2">
					{#each displayDealVoters().slice(0, maxVisibleVoters) as voter (voter)}
						<div class="rounded bg-green-500/20 px-3 py-1.5 text-sm font-medium text-white">
							{voter}
						</div>
					{/each}
					{#if displayDealVoters().length > maxVisibleVoters}
						<div class="rounded bg-green-500/30 px-3 py-1.5 text-sm font-medium text-white">
							+{displayDealVoters().length - maxVisibleVoters} more
						</div>
					{/if}
				</div>
			</div>
		</div>

		<!-- No Deal Column -->
		<div class="flex flex-col rounded-xl bg-red-500/10 p-4">
			<div class="mb-4 text-center">
				<div class="text-2xl font-bold text-red-300">❤️ No Deal?</div>
				<div class="text-lg text-red-200">({noDealCount} votes)</div>
			</div>
			<div class="flex-1 overflow-y-auto">
				<div class="flex flex-wrap gap-2">
					{#each displayNoDealVoters().slice(0, maxVisibleVoters) as voter (voter)}
						<div class="rounded bg-red-500/20 px-3 py-1.5 text-sm font-medium text-white">
							{voter}
						</div>
					{/each}
					{#if displayNoDealVoters().length > maxVisibleVoters}
						<div class="rounded bg-red-500/30 px-3 py-1.5 text-sm font-medium text-white">
							+{displayNoDealVoters().length - maxVisibleVoters} more
						</div>
					{/if}
				</div>
			</div>
		</div>
	</div>
</div>
