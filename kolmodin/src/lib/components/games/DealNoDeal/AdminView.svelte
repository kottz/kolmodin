<script lang="ts">
	import { dealNoDealStore } from './store.svelte';
	import { Button } from '$lib/components/ui/button'; // Corrected path
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { info, debug } from '$lib/utils/logger';
	import type { GamePhaseType } from './types';

	const dndState = $derived(dealNoDealStore.gameState);
	const liveVotes = $derived(dealNoDealStore.liveVoteFeed);

	const TOTAL_CASES = 22;
	const ALL_MONEY_VALUES: number[] = [
		1, 5, 10, 25, 50, 75, 100, 200, 300, 400, 500, 750, 1000, 5000, 10000, 25000, 50000, 75000,
		100000, 250000, 500000, 1000000
	].sort((a, b) => a - b);

	// This function is called from the template, so it's fine.
	function getPhaseDescription(phase: GamePhaseType): string {
		switch (phase.type) {
			case 'Setup':
				return 'Game Setup';
			case 'PlayerCaseSelection_Voting':
				return 'Player Choosing Their Case (Voting)';
			case 'RoundCaseOpening_Voting':
				return `Round ${phase.round_number}: Open ${phase.total_to_open_for_round - phase.opened_so_far_for_round} More Case(s) (Voting)`;
			case 'BankerOfferCalculation':
				return `Round ${phase.round_number}: Banker Calculating Offer...`;
			case 'DealOrNoDeal_Voting':
				return `Round ${phase.round_number}: Deal or No Deal? (Offer: $${phase.offer.toLocaleString()}) (Voting)`;
			case 'GameOver':
				return `Game Over: ${phase.summary}`;
			default:
				// Exhaustive check for unhandled phase types during development
				// const _exhaustiveCheck: never = phase;
				return 'Unknown Phase';
		}
	}

	// This function is called from the template, so it's fine.
	function isVotingPhaseActive(phase: GamePhaseType): boolean {
		return (
			phase.type === 'PlayerCaseSelection_Voting' ||
			phase.type === 'RoundCaseOpening_Voting' ||
			phase.type === 'DealOrNoDeal_Voting'
		);
	}

	let rawStateForDebug = $derived(JSON.stringify(dndState, null, 2));

	// Use $effect for logging reactive changes
	$effect(() => {
		debug('DealNoDeal AdminView rendered/updated. Phase:', dndState.phase.type);
		// You can log other reactive values here too if needed
		// debug('Current live votes count:', liveVotes.length);
	});

	info('DealNoDeal AdminView script executed (runs once on component init).');
</script>

<div class="space-y-6">
	<Card>
		<CardHeader>
			<CardTitle>Deal or No Deal - Admin Panel</CardTitle>
			<CardDescription>
				Phase: <span class="text-primary font-semibold">{getPhaseDescription(dndState.phase)}</span>
			</CardDescription>
		</CardHeader>
		<CardContent class="space-y-4">
			<div class="flex space-x-2">
				<!-- Use onclick for Svelte 5 event handling on components -->
				<Button
					onclick={dealNoDealStore.actions.startGame}
					disabled={!(dndState.phase.type === 'Setup' || dndState.phase.type === 'GameOver')}
				>
					{#if dndState.phase.type === 'GameOver'}Restart Game{:else}Start Game{/if}
				</Button>
				<Button
					onclick={dealNoDealStore.actions.concludeVotingAndProcess}
					disabled={!isVotingPhaseActive(dndState.phase)}
					variant="secondary"
				>
					Conclude Voting & Process
				</Button>
			</div>

			{#if dndState.phase.type !== 'Setup' && dndState.phase.type !== 'GameOver'}
				<p class="text-muted-foreground text-sm">
					Round: {dndState.current_round_display_number} | Cases to Open this Round: {dndState.cases_to_open_this_round_target}
					| Opened in Segment: {dndState.cases_opened_in_current_round_segment}
				</p>
				{#if dndState.banker_offer !== null && dndState.phase.type !== 'DealOrNoDeal_Voting'}
					<p class="text-lg font-semibold text-green-500">
						Current Banker Offer: ${dndState.banker_offer.toLocaleString()}
					</p>
				{/if}
			{/if}
		</CardContent>
	</Card>

	{#if dndState.phase.type !== 'Setup'}
		<div class="grid grid-cols-1 gap-6 md:grid-cols-3">
			<Card class="md:col-span-2">
				<CardHeader><CardTitle>Briefcases</CardTitle></CardHeader>
				<CardContent>
					{#if dndState.briefcase_values.length > 0}
						<div class="grid grid-cols-4 gap-2 sm:grid-cols-5 md:grid-cols-6 lg:grid-cols-7">
							{#each { length: TOTAL_CASES } as _, i (i)}
								{@const caseIndex = i}
								<!-- Ensure dndState properties are accessed safely, especially array indices -->
								{@const isOpened = dndState.briefcase_is_opened?.[caseIndex]}
								{@const value = dndState.briefcase_values?.[caseIndex]}
								{@const isPlayerCase = dndState.player_chosen_case_index === caseIndex}
								<div
									class="flex aspect-square flex-col items-center justify-center rounded border p-1 text-center text-xs shadow sm:p-2 sm:text-sm
                                    {isOpened
										? 'bg-muted text-muted-foreground line-through'
										: 'bg-background hover:bg-accent cursor-default'}
                                    {isPlayerCase && !isOpened
										? 'border-primary border-2 font-semibold'
										: 'border-border'}
                                    {isPlayerCase && isOpened && dndState.phase.type === 'GameOver'
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
				</CardContent>
			</Card>

			<div class="space-y-6">
				<Card>
					<CardHeader><CardTitle>Money Board</CardTitle></CardHeader>
					<CardContent>
						<div class="grid grid-cols-2 gap-x-4 gap-y-1 text-sm">
							{#each ALL_MONEY_VALUES as moneyValue (moneyValue)}
								<!-- Corrected class_name to class -->
								<div
									class={!dndState.remaining_money_values_in_play?.includes(moneyValue)
										? 'text-muted-foreground line-through'
										: moneyValue >= 100000
											? 'text-primary font-semibold'
											: ''}
								>
									${moneyValue.toLocaleString()}
								</div>
							{/each}
						</div>
					</CardContent>
				</Card>

				{#if isVotingPhaseActive(dndState.phase)}
					<Card>
						<CardHeader><CardTitle>Vote Status</CardTitle></CardHeader>
						<CardContent>
							<h4 class="mb-1 text-sm font-medium">Current Vote Tally:</h4>
							{#if dndState.current_vote_tally && Object.keys(dndState.current_vote_tally).length > 0}
								<ul class="mb-3 list-disc pl-5 text-sm">
									{#each Object.entries(dndState.current_vote_tally) as [vote, count] (vote)}
										<li>{vote}: {count}</li>
									{/each}
								</ul>
							{:else}
								<p class="text-muted-foreground mb-3 text-sm">Awaiting votes...</p>
							{/if}

							<h4 class="mb-1 text-sm font-medium">Live Vote Feed (Last {liveVotes.length}):</h4>
							{#if liveVotes.length > 0}
								<ScrollArea class="h-32 rounded-md border p-2 text-xs">
									{#each liveVotes as vote (vote.voter_username + vote.vote_value + Math.random())}
										<p>
											<span class="font-semibold">{vote.voter_username}:</span>
											{vote.vote_value}
										</p>
									{/each}
								</ScrollArea>
							{:else}
								<p class="text-muted-foreground text-xs">
									No live votes received yet for this segment.
								</p>
							{/if}
						</CardContent>
					</Card>
				{/if}
			</div>
		</div>
	{/if}

	<details class="mt-6">
		<summary class="text-muted-foreground cursor-pointer text-sm"
			>Show Raw Game State (Debug)</summary
		>
		<ScrollArea class="bg-muted mt-2 max-h-96 overflow-x-auto rounded p-3 text-xs">
			<pre>{rawStateForDebug}</pre>
		</ScrollArea>
	</details>
</div>
