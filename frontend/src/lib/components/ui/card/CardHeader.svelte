<script lang="ts">
	import type { HTMLDivAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	type $$Props = HTMLDivAttributes;

	let { class: className = undefined, children, ...rest }: $$Props = $props();

	const finalClass = $derived(cn('flex flex-col space-y-1.5 p-6 pb-4', className));
	// Adjusted padding slightly, common to have less bottom padding in header
</script>

<div class={finalClass} {...rest}>
	{@render children()}
</div>
