<script lang="ts">
	// For ClipQueue, the AdminView IS the stream display
	// Import and display the AdminView directly
	import AdminView from './AdminView.svelte';
</script>

<!-- ClipQueue uses the same interface for both admin and stream -->
<AdminView />
