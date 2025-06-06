<script lang="ts">
	import type { HTMLDivAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	type $$Props = HTMLDivAttributes;

	let { class: className = undefined, children, ...rest }: $$Props = $props();

	const finalClass = $derived(cn('flex items-center p-6 pt-4', className));
	// Adjusted padding slightly
</script>

<div class={finalClass} {...rest}>
	{@render children()}
</div>
