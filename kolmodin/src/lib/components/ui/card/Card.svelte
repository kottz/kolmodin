<script lang="ts">
	import type { HTMLDivAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	type $$Props = HTMLDivAttributes; // Accepts all standard div attributes

	let {
		class: className = undefined,
		children, // For the slot
		...rest
	}: $$Props = $props();

	const finalClass = $derived(
		cn('rounded-lg border border-border bg-card text-card-foreground shadow-md', className)
		// Using shadow-md as a default, can be overridden.
		// Ensure --border, --card, --card-foreground are defined in your CSS variables.
	);
</script>

<div class={finalClass} {...rest}>
	{@render children()}
</div>
