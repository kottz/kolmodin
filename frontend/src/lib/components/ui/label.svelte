<script lang="ts">
	import type { HTMLLabelAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	interface Props extends HTMLLabelAttributes {
		class?: string;
	}

	let { class: className, children, ...restProps }: Props = $props();
</script>

<label
	class={cn(
		'text-sm leading-none font-medium peer-disabled:cursor-not-allowed peer-disabled:opacity-70',
		className
	)}
	{...restProps}
>
	{@render children?.()}
</label>
