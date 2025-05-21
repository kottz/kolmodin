<script lang="ts">
	import { dealNoDealStore } from './store.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle,
		CardDescription
	} from '$lib/components/ui/card';
	import { info } from '$lib/utils/logger';
	import BriefcaseGrid from './components/BriefcaseGrid.svelte';

	const dndState = $derived(dealNoDealStore.gameState);
	const currentPhaseType = $derived(dealNoDealStore.gameState.phase.type);

	const moneyBoardColumns = $derived(() => {
		const sortedValues = [...(dndState.briefcase_values || [])].sort((a, b) => a - b);
		if (sortedValues.length === 0) {
			return { left: [], right: [] };
		}

		const midpoint = Math.ceil(sortedValues.length / 2);
		const left = sortedValues.slice(0, midpoint);
		const right = sortedValues.slice(midpoint);
		return { left, right };
	});

	const currentAdminPanelOffer = $derived(() => {
		if (dndState.phase.type === 'DealOrNoDeal_Voting') {
			return dndState.phase.offer;
		}
		return dndState.banker_offer;
	});

	function handleLeaveGame() {
		info('Leave Game button clicked');
		// Example: window.location.href = '/lobby';
	}

	info('DealNoDeal AdminView script executed.');
</script>

<div class="relative min-h-screen bg-background p-4 md:p-6">
	<div class="absolute left-4 top-4 md:left-6 md:top-6">
		<Button variant="outline" onclick={handleLeaveGame}>Leave Game</Button>
	</div>

	<div class="mx-auto max-w-6xl space-y-6 pt-16 md:pt-20">
		<Card>
			<CardHeader>
				<CardTitle>Deal or No Deal - Admin Panel</CardTitle>
				<CardDescription>
					Phase: <span class="text-primary font-semibold">{currentPhaseType}</span>
				</CardDescription>
			</CardHeader>
			<CardContent class="space-y-4">
				<div class="flex flex-wrap gap-2">
					<Button onclick={dealNoDealStore.actions.startGame} size="sm">
						{#if dndState.phase.type === 'GameOver'}Restart Game{:else}Start Game{/if}
					</Button>
					<Button
						onclick={dealNoDealStore.actions.concludeVotingAndProcess}
						variant="secondary"
						size="sm"
					>
						Conclude Voting & Process
					</Button>
				</div>

				{#if dndState.phase.type !== 'Setup' && dndState.phase.type !== 'GameOver'}
					<p class="text-muted-foreground text-sm">
						Round: {dndState.current_round_display_number} | Cases to Open:
						{dndState.cases_to_open_this_round_target} | Opened This Round:
						{dndState.cases_opened_in_current_round_segment}
					</p>
					{#if currentAdminPanelOffer !== null}
						<p class="text-lg font-semibold text-green-600 dark:text-green-400">
							Banker Offer: ${currentAdminPanelOffer.toLocaleString()}
						</p>
					{/if}
				{/if}
			</CardContent>
		</Card>

		{#if dndState.phase.type !== 'Setup'}
			<div class="grid grid-cols-1 gap-6 lg:grid-cols-3">
				<Card class="lg:col-span-2">
					<CardHeader><CardTitle>Briefcases</CardTitle></CardHeader>
					<CardContent>
						<BriefcaseGrid
							values={dndState.briefcase_values || []}
							isOpenedStates={dndState.briefcase_is_opened || []}
							playerChosenIndex={dndState.player_chosen_case_index}
							phaseType={currentPhaseType}
						/>
					</CardContent>
				</Card>

				<div class="space-y-6 lg:col-span-1">
					<Card>
						<CardHeader><CardTitle>Money Board</CardTitle></CardHeader>
						<CardContent>
							{#if moneyBoardColumns().left.length > 0}
								<div class="grid grid-cols-2 gap-x-3 gap-y-1.5 text-xs sm:text-sm">
									<div class="flex flex-col space-y-1.5">
										{#each moneyBoardColumns().left as moneyValue (moneyValue)}
											{@const isActive =
												dndState.remaining_money_values_in_play?.includes(moneyValue)}
											<div
												class="rounded px-2 py-1.5 text-center font-medium shadow-sm
                                               {isActive
													? moneyValue >= 100000
														? 'bg-red-500 text-white dark:bg-red-600'
														: 'bg-blue-500 text-white dark:bg-blue-600'
													: 'bg-muted text-muted-foreground line-through opacity-60 dark:bg-neutral-700 dark:text-neutral-400'}"
											>
												${moneyValue.toLocaleString()}
											</div>
										{/each}
									</div>
									<div class="flex flex-col space-y-1.5">
										{#each moneyBoardColumns().right as moneyValue (moneyValue)}
											{@const isActive =
												dndState.remaining_money_values_in_play?.includes(moneyValue)}
											<div
												class="rounded px-2 py-1.5 text-center font-medium shadow-sm
                                               {isActive
													? moneyValue >= 100000
														? 'bg-red-500 text-white dark:bg-red-600'
														: 'bg-blue-500 text-white dark:bg-blue-600'
													: 'bg-muted text-muted-foreground line-through opacity-60 dark:bg-neutral-700 dark:text-neutral-400'}"
											>
												${moneyValue.toLocaleString()}
											</div>
										{/each}
									</div>
								</div>
							{:else}
								<p class="text-muted-foreground">Money values not yet available.</p>
							{/if}
						</CardContent>
					</Card>
				</div>
			</div>
		{/if}
	</div>
</div>
