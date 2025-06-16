<script lang="ts">
	import { dealNoDealStore } from './store.svelte';
	import { Button } from '$lib/components/ui/button';
	import StreamControls from '$lib/components/ui/StreamControls.svelte';
	import {
		Card,
		CardContent,
		CardHeader,
		CardTitle
		// CardDescription component is no longer used in this specific file after changes
	} from '$lib/components/ui/card';
	import BriefcaseGrid from './components/BriefcaseGrid.svelte';

	const dndState = $derived(dealNoDealStore.gameState);
	// currentPhaseType is used for the fallback message in the Player Information Card
	//const currentPhaseType = $derived(dndState.phase.type);
	const currentPhaseType = $derived(dealNoDealStore.gameState.phase.type);
	const dealNoDealMap = $derived(dealNoDealStore.dealNoDealVotesMap);
	const switchKeepMap = $derived(dealNoDealStore.switchKeepVotesMap);

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

	// The currentAdminPanelOffer derived state is no longer needed as this info
	// is now displayed in the new Player Information Card or not at all in the admin panel.
</script>

<div class="bg-background relative min-h-screen">
	<div class="mx-auto mt-6 px-4 sm:px-6 lg:px-8">
		<!-- Player Information Card -->
		<div class="mb-6">
			<Card>
				<CardContent class="p-8 text-center">
					{#if dndState.phase.type === 'Setup'}
						<h2 class="text-4xl font-bold">Waiting to start the game</h2>
					{:else if dndState.phase.type === 'PlayerCaseSelectionVoting'}
						<h2 class="text-4xl font-bold">Vote for your briefcase</h2>
					{:else if dndState.phase.type === 'RoundCaseOpeningVoting'}
						<h2 class="text-3xl font-bold">
							{dndState.phase.data.opened_so_far_for_round} / {dndState.phase.data
								.total_to_open_for_round} cases opened this round
						</h2>
					{:else if dndState.phase.type === 'BankerOfferCalculation'}
						<h2 class="text-muted-foreground text-3xl font-bold">
							Banker is calculating the offer...
						</h2>
					{:else if dndState.phase.type === 'DealOrNoDealVoting'}
						<!-- Visual Deal/No Deal Voting Display -->
						<div class="grid grid-cols-3 items-start gap-8">
							<!-- DEAL Column -->
							<div class="text-center">
								<h3 class="mb-4 text-2xl font-bold">DEAL</h3>
								<div class="space-y-2">
									{#each dealNoDealMap['DEAL'] || [] as player (player)}
										<div class="text-lg">{player}</div>
									{/each}
									{#if !dealNoDealMap['DEAL'] || dealNoDealMap['DEAL'].length === 0}
										<div class="text-muted-foreground italic">No votes yet</div>
									{/if}
								</div>
							</div>

							<!-- Bank Offer Column -->
							<div class="text-center">
								<h3 class="mb-2 text-xl font-semibold">Bank offer</h3>
								<div class="text-4xl font-bold">
									${dndState.phase.data.offer.toLocaleString()}
								</div>
							</div>

							<!-- NO DEAL Column -->
							<div class="text-center">
								<h3 class="mb-4 text-2xl font-bold">NO DEAL</h3>
								<div class="space-y-2">
									{#each dealNoDealMap['NO DEAL'] || [] as player (player)}
										<div class="text-lg">{player}</div>
									{/each}
									{#if !dealNoDealMap['NO DEAL'] || dealNoDealMap['NO DEAL'].length === 0}
										<div class="text-muted-foreground italic">No votes yet</div>
									{/if}
								</div>
							</div>
						</div>
					{:else if dndState.phase.type === 'SwitchOrKeepVoting'}
						<!-- Visual Switch/Keep Voting Display -->
						<div class="grid grid-cols-3 items-start gap-8">
							<!-- SWITCH Column -->
							<div class="text-center">
								<h3 class="mb-4 text-2xl font-bold">SWITCH</h3>
								<div class="space-y-2">
									{#each switchKeepMap['SWITCH'] || [] as player (player)}
										<div class="text-lg">{player}</div>
									{/each}
									{#if !switchKeepMap['SWITCH'] || switchKeepMap['SWITCH'].length === 0}
										<div class="text-muted-foreground italic">No votes yet</div>
									{/if}
								</div>
							</div>

							<!-- Final Decision Column -->
							<div class="text-center">
								<h3 class="mb-2 text-xl font-semibold">Final Decision</h3>
								<div class="mb-2 text-2xl font-bold">
									Your Case #{(dndState.player_chosen_case_index || 0) + 1}
								</div>
								<div class="text-muted-foreground mb-2 text-lg">vs</div>
								<div class="text-2xl font-bold">
									Case #{dndState.phase.data.final_case_index + 1}
								</div>
							</div>

							<!-- KEEP Column -->
							<div class="text-center">
								<h3 class="mb-4 text-2xl font-bold">KEEP</h3>
								<div class="space-y-2">
									{#each switchKeepMap['KEEP'] || [] as player (player)}
										<div class="text-lg">{player}</div>
									{/each}
									{#if !switchKeepMap['KEEP'] || switchKeepMap['KEEP'].length === 0}
										<div class="text-muted-foreground italic">No votes yet</div>
									{/if}
								</div>
							</div>
						</div>
					{:else if dndState.phase.type === 'GameOver'}
						<h2 class="text-4xl font-bold">
							Game Over. {#if dndState.phase.data.winnings > 0}You won ${dndState.phase.data.winnings.toLocaleString()}!{:else}Better
								luck next time!{/if}
						</h2>
					{:else}
						<!-- Fallback for any other phase -->
						<h2 class="text-2xl font-bold">Loading game state... ({currentPhaseType})</h2>
					{/if}
				</CardContent>
			</Card>
		</div>

		<div class="grid grid-cols-1 gap-6 lg:grid-cols-3">
			<!-- Left Column -->
			<div class="space-y-6 lg:col-span-1">
				<!-- Money Board Card (conditionally rendered) -->
				{#if dndState.phase.type !== 'Setup'}
					<Card>
						<CardHeader><CardTitle>Money Board</CardTitle></CardHeader>
						<CardContent>
							{#if moneyBoardColumns().left.length > 0}
								<div class="grid grid-cols-2 gap-x-3 gap-y-1.5 text-xl sm:text-xl">
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
				{/if}

				<!-- Admin Panel Card -->
				<Card>
					<CardHeader>
						<CardTitle>Admin Controls</CardTitle>
						<!-- CardDescription with Phase info removed as per request -->
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
						<!-- Detailed game state info (round, cases to open, banker offer) removed from here -->
					</CardContent>
				</Card>

				<!-- Stream Controls Card -->
				<StreamControls />
			</div>

			<!-- Right Column -->
			<div class="lg:col-span-2">
				<!-- Briefcases Card (conditionally rendered) -->
				{#if dndState.phase.type !== 'Setup'}
					<Card>
						<CardHeader><CardTitle>Briefcases</CardTitle></CardHeader>
						<CardContent>
							<BriefcaseGrid />
						</CardContent>
					</Card>
				{/if}
			</div>
		</div>
	</div>
</div>
