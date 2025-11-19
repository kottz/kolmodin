<script lang="ts">
	import { notificationStore, type Notification } from '$lib/stores/notification.store.svelte';
	import { Button } from '$lib/components/ui/button'; // Assuming you'll have a styled Button
	import { X } from 'lucide-svelte'; // For a close icon
	import { fly } from 'svelte/transition';
	import { flip } from 'svelte/animate'; // For smoother reordering if notifications are removed out of order

	// Reactive access to the notifications array
	const notifications = $derived(notificationStore.current);

	function getVariantClass(type: Notification['type']): string {
		switch (type) {
			case 'success':
				return 'bg-green-100 border-green-500 text-green-700 dark:bg-green-900/30 dark:border-green-700 dark:text-green-300';
			case 'destructive':
				return 'bg-red-100 border-red-500 text-red-700 dark:bg-red-900/30 dark:border-red-700 dark:text-red-300';
			case 'warning':
				return 'bg-yellow-100 border-yellow-500 text-yellow-700 dark:bg-yellow-900/30 dark:border-yellow-600 dark:text-yellow-300';
			case 'info':
				return 'bg-blue-100 border-blue-500 text-blue-700 dark:bg-blue-900/30 dark:border-blue-700 dark:text-blue-300';
			default: // 'default'
				return 'bg-gray-100 border-gray-400 text-gray-800 dark:bg-gray-700/50 dark:border-gray-500 dark:text-gray-200';
		}
	}

	function getIconColorClass(type: Notification['type']): string {
		switch (type) {
			case 'success':
				return 'text-green-500 dark:text-green-400';
			case 'destructive':
				return 'text-red-500 dark:text-red-400';
			case 'warning':
				return 'text-yellow-500 dark:text-yellow-400';
			case 'info':
				return 'text-blue-500 dark:text-blue-400';
			default:
				return 'text-gray-500 dark:text-gray-400';
		}
	}
</script>

{#if notifications.length > 0}
	<div
		class="fixed right-4 bottom-4 z-100 flex flex-col items-end space-y-2 sm:top-4 sm:right-4 sm:bottom-auto md:top-6 md:right-6"
		aria-live="assertive"
	>
		{#each notifications as notification (notification.id)}
			<div
				role="alert"
				class="pointer-events-auto w-full max-w-sm overflow-hidden rounded-lg border shadow-lg {getVariantClass(
					notification.type
				)}"
				in:fly={{ x: 300, duration: 300, delay: 100 }}
				out:fly={{ x: 300, duration: 200 }}
				animate:flip={{ duration: 250 }}
			>
				<div class="flex p-3 pr-2">
					<div class="flex-1">
						<!-- Optional: Add icons based on type -->
						<!-- <svelte:component this={getIcon(notification.type)} class="h-5 w-5 {getIconColorClass(notification.type)} mr-2 mt-0.5" /> -->
						<p class="text-sm font-medium">{notification.message}</p>
					</div>
					<div class="ml-2 shrink-0">
						<Button
							variant="ghost"
							size="icon"
							class="inline-flex h-7 w-7 rounded-md p-1.5 {getIconColorClass(
								notification.type
							)} hover:bg-black/10 dark:hover:bg-white/10"
							on:click={() => notificationStore.remove(notification.id)}
							aria-label="Close notification"
						>
							<X class="h-4 w-4" />
						</Button>
					</div>
				</div>
			</div>
		{/each}
	</div>
{/if}

<style lang="postcss">
	/* Add any specific styles here if needed, though Tailwind should cover most */
</style>
