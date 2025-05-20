import { log } from '$lib/utils/logger';

export type NotificationType = 'default' | 'success' | 'destructive' | 'warning' | 'info';

export interface Notification {
	id: number;
	message: string;
	type: NotificationType;
	duration?: number; // Optional duration in ms, defaults to DEFAULT_DURATION
}

const DEFAULT_DURATION = 5000; // 5 seconds

// No explicit NotificationStore interface needed as we export the store object directly.

function createNotificationStore() {
	const notifications = $state<Notification[]>([]);
	let nextId = $state(0); // Using $state for nextId too, though simple counter would also work

	function add(
		message: string,
		type: NotificationType = 'default',
		duration: number = DEFAULT_DURATION
	): number {
		const id = nextId++; // Increment and assign
		const newNotification: Notification = { id, message, type, duration };

		log(`Notification added (ID: ${id}, Type: ${type}): "${message}"`);
		notifications.push(newNotification); // Directly mutate the $state array

		if (duration > 0) {
			// Allow duration 0 or negative for persistent notifications
			setTimeout(() => {
				remove(id);
			}, duration);
		}
		return id;
	}

	function remove(id: number): void {
		const index = notifications.findIndex((n) => n.id === id);
		if (index > -1) {
			notifications.splice(index, 1); // Directly mutate the $state array
			log(`Notification removed (ID: ${id})`);
		}
	}

	function clearAll(): void {
		log('All notifications cleared.');
		notifications.length = 0; // Clear the array
		// or notifications = []; if you prefer reassignment, though .length = 0 is fine for $state arrays.
	}

	return {
		// Expose the reactive state directly for components to read
		get current() {
			// Renamed from 'subscribe' to 'current' or 'list' or similar for rune-based stores
			return notifications;
		},
		add,
		remove,
		clearAll
	};
}

export const notificationStore = createNotificationStore();

// Example Usage in a Svelte component:
// <script lang="ts">
//     import { notificationStore } from '$lib/stores/notification.store';
//
//     // To display notifications:
//     // const notifications = $derived(notificationStore.current);
//     // or directly in template: {#each notificationStore.current as notification}
//
//     function showSuccess() {
//         notificationStore.add('Operation successful!', 'success');
//     }
// </script>
//
// {#each notificationStore.current as notification (notification.id)}
//   <div class="notification {notification.type}">{notification.message}</div>
// {/each}
