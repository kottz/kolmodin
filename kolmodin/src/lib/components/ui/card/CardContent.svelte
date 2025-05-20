<script lang="ts">
	import type { HTMLDivAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	type $$Props = HTMLDivAttributes;

	let { class: className = undefined, children, ...rest }: $$Props = $props();

	const finalClass = $derived(cn('p-6 pt-0', className)); // pt-0 if header has pb-4
</script>

<div class={finalClass} {...rest}>
	{@render children()}
</div>
