<script lang="ts">
	// If you need more specific type checking for phaseType, you could import GamePhaseType['type']
	// For now, string is sufficient for its usage here.

	type Props = {
		values: number[];
		isOpenedStates: boolean[];
		playerChosenIndex: number | null;
		phaseType: string; // e.g., 'Setup', 'GameOver', 'RoundCaseOpening_Voting'
	};

	let { values, isOpenedStates, playerChosenIndex, phaseType }: Props = $props();
</script>

{#if values.length > 0}
	<div class="grid grid-cols-4 gap-2 sm:grid-cols-5 md:grid-cols-6 lg:grid-cols-7 xl:grid-cols-9">
		{#each { length: values.length } as _, i (i)}
			{@const caseIndex = i}
			{@const isOpened = isOpenedStates?.[caseIndex]}
			{@const value = values?.[caseIndex]}
			{@const isPlayerCase = playerChosenIndex === caseIndex}
			<div
				class="flex aspect-square flex-col items-center justify-center rounded border p-1 text-center text-xs shadow-sm sm:p-2 sm:text-sm
                {isOpened
					? 'bg-muted text-muted-foreground line-through opacity-70'
					: 'bg-background hover:bg-accent cursor-default'}
                {isPlayerCase && !isOpened
					? 'border-primary ring-2 ring-primary ring-offset-2 ring-offset-background font-semibold'
					: 'border-border'}
                {isPlayerCase && isOpened && phaseType === 'GameOver'
					? 'border-2 border-amber-500 font-semibold'
					: ''}"
				title={isPlayerCase && !isOpened
					? "Player's Case"
					: isOpened
						? `Opened: $${value?.toLocaleString() ?? 'N/A'}`
						: `Case #${caseIndex + 1}`}
			>
				<span class="font-bold">#{caseIndex + 1}</span>
				{#if isOpened && value !== undefined}
					<span class="mt-0.5 text-[0.6rem] sm:text-xs">${value.toLocaleString()}</span>
				{/if}
				{#if isPlayerCase && !isOpened}
					<span class="mt-0.5 text-[0.6rem] sm:text-xs">(Your Pick)</span>
				{/if}
			</div>
		{/each}
	</div>
{:else}
	<p class="text-muted-foreground">Briefcases not yet initialized.</p>
{/if}
