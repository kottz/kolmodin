<script lang="ts">
	import type { InputProps } from './index'; // Import from our index.ts
	import { cn } from '$lib/utils/cn';
	// import { tv, type VariantProps } from 'tailwind-variants'; // Optional for variants

	type $$Props = InputProps;
	// In Svelte 5 runes mode, explicit $$Events isn't typically needed for forwarding native events
	// if they are handled via {...rest} or direct on:event listeners.

	let {
		class: className = undefined,
		value = $bindable(), // Use $bindable for two-way binding with bind:value
		type = 'text', // Default type
		...rest // Captures all other HTMLInputAttributes including event handlers like oninput, onclick
	}: $$Props = $props();

	// Optional: Define variants using tailwind-variants if needed
	// const inputVariants = tv({
	//  base: 'flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50',
	//  variants: {
	//      error: {
	//          true: 'border-destructive focus-visible:ring-destructive text-destructive placeholder:text-destructive/70',
	//      },
	//      inputSize: { // Renamed from 'size' to avoid conflict with HTML attribute
	//          default: 'h-10 py-2 text-sm',
	//          sm: 'h-9 py-1.5 text-xs',
	//          lg: 'h-11 py-3 text-base'
	//      }
	//  },
	//  defaultVariants: {
	//      inputSize: "default"
	//  }
	// });
	// const finalClass = $derived(cn(inputVariants({ error: rest.error, inputSize: rest.inputSize }), className));
	// Note: If using variants, you'd add 'error' and 'inputSize' to InputProps and destructure them.

	const baseInputClasses =
		'flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50';

	const finalClass = $derived(cn(baseInputClasses, className));
</script>

<input bind:value {type} class={finalClass} {...rest} />
