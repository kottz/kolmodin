<script lang="ts">
	import type { HTMLHeadingAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';
	import type { HeadingLevel } from './index';

	type $$Props = HTMLHeadingAttributes & {
		tag?: HeadingLevel;
	};

	let {
		tag: Tag = 'h3', // Default to <h3>
		class: className = undefined,
		children,
		...rest
	}: $$Props = $props();

	const finalClass = $derived(
		cn('text-xl font-semibold leading-none tracking-tight text-card-foreground', className)
		// Example: text-xl, adjust as per your design system
	);
</script>

<svelte:element this={Tag} class={finalClass} {...rest}>
	{@render children()}
</svelte:element>
