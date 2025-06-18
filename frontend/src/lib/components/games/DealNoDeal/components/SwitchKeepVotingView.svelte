<script lang="ts">
	import type { DealNoDealPublicState } from '../types';

	interface Props {
		gameState: DealNoDealPublicState;
	}

	let { gameState }: Props = $props();

	// Extract switch/keep voting data
	const switchKeepData = $derived(
		gameState.phase.type === 'SwitchOrKeepVoting' ? gameState.phase.data : null
	);

	const finalCaseIndex = $derived(switchKeepData?.final_case_index ?? null);
	const playerCaseIndex = $derived(gameState.playerChosenCaseIndex);

	// Get vote counts and voter names from gameState
	const switchCount = $derived(gameState.switchKeepVotes?.switch?.length || 0);
	const keepCount = $derived(gameState.switchKeepVotes?.keep?.length || 0);
	const displaySwitchVoters = $derived(() => gameState.switchKeepVotes?.switch || []);
	const displayKeepVoters = $derived(() => gameState.switchKeepVotes?.keep || []);

	const maxVisibleVoters = 8;
</script>

<div class="h-full rounded-2xl bg-black/30 p-6 backdrop-blur-sm">
	<!-- Top Title Bar with Case Information -->
	<div class="mb-6 text-center">
		<div class="mb-4 text-lg text-white/80">🔄 Final Decision Time</div>
		<div class="grid grid-cols-3 items-center gap-6">
			<!-- Your Original Case -->
			<div class="text-center">
				<div class="text-sm text-blue-200">Your Original Case</div>
				<div class="text-3xl font-bold text-blue-300">
					#{(playerCaseIndex || 0) + 1}
				</div>
			</div>

			<!-- VS -->
			<div class="text-center">
				<div class="text-2xl font-bold text-white/60">VS</div>
			</div>

			<!-- Alternative Case -->
			<div class="text-center">
				<div class="text-sm text-purple-200">Alternative Case</div>
				<div class="text-3xl font-bold text-purple-300">
					#{(finalCaseIndex || 0) + 1}
				</div>
			</div>
		</div>
	</div>

	<!-- Voting Columns -->
	<div class="grid h-[calc(100%-10rem)] grid-cols-2 gap-6">
		<!-- Keep Column -->
		<div class="flex flex-col rounded-xl bg-blue-500/10 p-4">
			<div class="mb-4 text-center">
				<div class="text-2xl font-bold text-blue-300">🛡️ Keep</div>
				<div class="text-sm text-blue-200">Stay with Case #{(playerCaseIndex || 0) + 1}</div>
				<div class="text-lg text-blue-200">({keepCount} votes)</div>
			</div>
			<div class="flex-1 overflow-y-auto">
				<div class="flex flex-wrap gap-2">
					{#each displayKeepVoters().slice(0, maxVisibleVoters) as voter (voter)}
						<div class="rounded bg-blue-500/20 px-3 py-1.5 text-sm font-medium text-white">
							{voter}
						</div>
					{/each}
					{#if displayKeepVoters().length > maxVisibleVoters}
						<div class="rounded bg-blue-500/30 px-3 py-1.5 text-sm font-medium text-white">
							+{displayKeepVoters().length - maxVisibleVoters} more
						</div>
					{/if}
				</div>
			</div>
		</div>

		<!-- Switch Column -->
		<div class="flex flex-col rounded-xl bg-purple-500/10 p-4">
			<div class="mb-4 text-center">
				<div class="text-2xl font-bold text-purple-300">🔄 Switch</div>
				<div class="text-sm text-purple-200">Switch to Case #{(finalCaseIndex || 0) + 1}</div>
				<div class="text-lg text-purple-200">({switchCount} votes)</div>
			</div>
			<div class="flex-1 overflow-y-auto">
				<div class="flex flex-wrap gap-2">
					{#each displaySwitchVoters().slice(0, maxVisibleVoters) as voter (voter)}
						<div class="rounded bg-purple-500/20 px-3 py-1.5 text-sm font-medium text-white">
							{voter}
						</div>
					{/each}
					{#if displaySwitchVoters().length > maxVisibleVoters}
						<div class="rounded bg-purple-500/30 px-3 py-1.5 text-sm font-medium text-white">
							+{displaySwitchVoters().length - maxVisibleVoters} more
						</div>
					{/if}
				</div>
			</div>
		</div>
	</div>
</div>
