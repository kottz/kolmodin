<script lang="ts">
	import { dealNoDealStore } from '../store.svelte'; // Adjust path if necessary

	// Access state directly from the store
	const dndState = $derived(dealNoDealStore.gameState);
	// Get the caseVotesMap directly from the store
	const caseVotesMapFromStore = $derived(dealNoDealStore.caseVotesMap);

	// Basic states needed for briefcase rendering
	const values = $derived(dndState.briefcase_values || []);
	const isOpenedStates = $derived(dndState.briefcase_is_opened || []);
	const playerChosenIndex = $derived(dndState.player_chosen_case_index);
	const phaseType = $derived(dndState.phase.type);
</script>

{#if values.length > 0}
	<div class="grid grid-cols-4 gap-2 sm:grid-cols-5 md:grid-cols-6 lg:grid-cols-7 xl:grid-cols-9">
		{#each { length: values.length } as _, i (i)}
			{@const caseIndex = i}
			{@const isOpened = isOpenedStates?.[caseIndex]}
			{@const caseValue = values?.[caseIndex]}
			{@const isPlayerCase = playerChosenIndex === caseIndex}
			{@const votersForThisCase = caseVotesMapFromStore[caseIndex]}

			<div
				class="flex aspect-square flex-col items-center justify-start rounded border p-1 text-center text-xs shadow-sm sm:p-2 sm:text-sm"
				class:bg-muted={isOpened}
				class:text-muted-foreground={isOpened}
				class:line-through={isOpened}
				class:opacity-70={isOpened}
				class:bg-background={!isOpened}
				class:hover:bg-accent={!isOpened}
				class:cursor-default={!isOpened}
				class:border-primary={isPlayerCase && !isOpened}
				class:ring-2={isPlayerCase && !isOpened}
				class:ring-primary={isPlayerCase && !isOpened}
				class:ring-offset-2={isPlayerCase && !isOpened}
				class:ring-offset-background={isPlayerCase && !isOpened}
				class:font-semibold={isPlayerCase &&
					!isOpened &&
					!(isPlayerCase && isOpened && phaseType === 'GameOver')}
				class:border-border={!(isPlayerCase && !isOpened) &&
					!(isPlayerCase && isOpened && phaseType === 'GameOver')}
				class:border-2={isPlayerCase && isOpened && phaseType === 'GameOver'}
				class:border-amber-500={isPlayerCase && isOpened && phaseType === 'GameOver'}
				title={isPlayerCase && !isOpened
					? "Player's Case"
					: isOpened
						? `Opened: $${caseValue?.toLocaleString() ?? 'N/A'}`
						: `Case #${caseIndex + 1}`}
			>
				<div>
					<div class="flex-shrink-0">
						<span class="font-bold {isPlayerCase && !isOpened ? '' : 'text-base sm:text-lg'}">
							#{caseIndex + 1}
						</span>
						{#if isPlayerCase && !isOpened}
							<span class="block text-[0.6rem] sm:text-xs">(Your Pick)</span>
						{/if}
					</div>

					{#if isOpened && caseValue !== undefined}
						<span class="mt-0.5 block text-[0.6rem] sm:text-xs">${caseValue.toLocaleString()}</span>
					{/if}
				</div>

				{#if !isOpened && votersForThisCase && votersForThisCase.length > 0}
					<div class="mt-auto max-h-[45%] w-full overflow-hidden pt-0.5">
						<div
							class="h-full overflow-y-auto rounded-sm bg-slate-200 p-0.5 text-slate-800 dark:bg-slate-700 dark:text-slate-100"
						>
							<p class="mb-0.5 text-[0.6rem] leading-tight font-semibold sm:text-[0.65rem]">
								Votes:
							</p>
							{#each votersForThisCase as voter (voter)}
								<span class="block truncate text-[0.55rem] leading-snug sm:text-[0.6rem]">
									{voter}
								</span>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		{/each}
	</div>
{:else}
	<p class="text-muted-foreground">Briefcases not yet initialized.</p>
{/if}
