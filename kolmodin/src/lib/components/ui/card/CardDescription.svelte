<script lang="ts">
	import type { HTMLParagraphAttributes } from 'svelte/elements';
	import { cn } from '$lib/utils/cn';

	type $$Props = HTMLParagraphAttributes;

	let { class: className = undefined, children, ...rest }: $$Props = $props();

	const finalClass = $derived(cn('text-sm text-muted-foreground', className));
</script>

<p class={finalClass} {...rest}>
	{@render children()}
</p>
