<!-- src/lib/components/games/Quiz/RecentGuesses.svelte -->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import {
		Card,
		CardContent,
		CardDescription,
		CardHeader,
		CardTitle
	} from '$lib/components/ui/card';
	import Badge from '$lib/components/ui/badge.svelte';
	import type { RecentGuess } from './types';

	interface Props {
		recentGuesses: RecentGuess[];
		onRemoveGuess: (guessId: string) => void;
	}

	let { recentGuesses, onRemoveGuess }: Props = $props();

	function formatTimestamp(timestamp: number): string {
		const date = new Date(timestamp * 1000);
		const now = new Date();
		const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);

		if (diffInSeconds < 60) {
			return `${diffInSeconds}s ago`;
		} else if (diffInSeconds < 3600) {
			const minutes = Math.floor(diffInSeconds / 60);
			return `${minutes}m ago`;
		} else {
			const hours = Math.floor(diffInSeconds / 3600);
			return `${hours}h ago`;
		}
	}

	function handleRemoveClick(guessId: string) {
		onRemoveGuess(guessId);
	}
</script>

<Card>
	<CardHeader>
		<CardTitle>Recent Answers</CardTitle>
		<CardDescription>
			{#if recentGuesses.length === 0}
				No recent answers yet
			{:else}
				Last {recentGuesses.length} correct answers - click X to remove and deduct point
			{/if}
		</CardDescription>
	</CardHeader>
	<CardContent>
		{#if recentGuesses.length === 0}
			<div class="text-muted-foreground py-8 text-center">
				<div class="mb-4 text-4xl">📝</div>
				<p>Recent correct answers will appear here</p>
			</div>
		{:else}
			<div class="space-y-3">
				{#each recentGuesses as guess (guess.id)}
					<div class="bg-muted/50 flex items-center justify-between rounded-lg border p-3">
						<div class="flex-1 space-y-1">
							<div class="flex items-center gap-2">
								<Badge variant="outline" class="text-xs">
									{guess.player}
								</Badge>
								<span class="text-muted-foreground text-xs">
									{formatTimestamp(guess.timestamp)}
								</span>
							</div>
							<div class="text-sm">
								Typed: <span class="bg-muted rounded px-1 py-0.5 font-mono text-xs"
									>"{guess.guessed_text}"</span
								>
							</div>
							<div class="text-sm">
								Question: <span class="bg-muted rounded px-1 py-0.5 font-mono text-xs font-medium"
									>"{guess.question}"</span
								>
							</div>
							<div class="text-sm">
								Answer: <span class="bg-muted rounded px-1 py-0.5 font-mono text-xs font-medium"
									>"{guess.correct_answer}"</span
								>
							</div>
						</div>
						<Button
							variant="ghost"
							size="sm"
							onclick={() => handleRemoveClick(guess.id)}
							class="text-destructive hover:text-destructive ml-2 h-8 w-8 p-0"
							title="Remove answer and deduct 1 point from {guess.player}"
						>
							✕
						</Button>
					</div>
				{/each}
			</div>
		{/if}
	</CardContent>
</Card>
