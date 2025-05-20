<script lang="ts">
	import { ScrollArea as ScrollAreaPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils/cn';
	import type { ScrollAreaProps } from './index';

	type $$Props = ScrollAreaProps;

	let {
		class: className = undefined,
		viewportClass = undefined,
		scrollbarXClass = undefined,
		scrollbarYClass = undefined,
		thumbXClass = undefined,
		thumbYClass = undefined,
		cornerClass = undefined,
		orientation = 'vertical',
		children,
		...restRootProps
	}: $$Props = $props();

	// Base Tailwind classes (customize these for your theme)
	// These are just examples; you'll likely use your theme's color palette
	const defaultScrollbarBaseClasses =
		'flex touch-none select-none transition-colors ease-out duration-150';
	const defaultVerticalScrollbarClasses = 'h-full w-2.5 border-l border-l-transparent p-px';
	const defaultHorizontalScrollbarClasses = 'h-2.5 w-full border-t border-t-transparent p-px';
	const defaultThumbBaseClasses = 'relative flex-1 rounded-full bg-border'; // e.g., using your 'border' color for the thumb
	const defaultCornerClasses = 'bg-border'; // e.g., using your 'border' color for the corner

	// Example classes for scrollbar visibility and hover, utilizing data attributes
	// These would be part of your theme or directly in scrollbarXClass/scrollbarYClass if customized per instance
	const scrollbarStateAndHoverClasses = 'data-[state=visible]:bg-muted/40 hover:bg-muted/60'; // Example for when scrollbar is visible or hovered
	// Add animate-in/out classes here if using tailwindcss-animate
	// e.g., 'data-[state=visible]:animate-in data-[state=visible]:fade-in-0 data-[state=hidden]:animate-out data-[state=hidden]:fade-out-0'
</script>

<ScrollAreaPrimitive.Root class={cn('relative overflow-hidden', className)} {...restRootProps}>
	<ScrollAreaPrimitive.Viewport class={cn('h-full w-full rounded-[inherit]', viewportClass)}>
		{@render children()}
	</ScrollAreaPrimitive.Viewport>

	{#if orientation === 'vertical' || orientation === 'both'}
		<ScrollAreaPrimitive.Scrollbar
			orientation="vertical"
			class={cn(
				defaultScrollbarBaseClasses,
				defaultVerticalScrollbarClasses,
				scrollbarStateAndHoverClasses, // Apply common state/hover classes
				scrollbarYClass // User-provided overrides/additions
			)}
		>
			<ScrollAreaPrimitive.Thumb class={cn(defaultThumbBaseClasses, thumbYClass)} />
		</ScrollAreaPrimitive.Scrollbar>
	{/if}

	{#if orientation === 'horizontal' || orientation === 'both'}
		<ScrollAreaPrimitive.Scrollbar
			orientation="horizontal"
			class={cn(
				defaultScrollbarBaseClasses,
				defaultHorizontalScrollbarClasses,
				scrollbarStateAndHoverClasses, // Apply common state/hover classes
				scrollbarXClass // User-provided overrides/additions
			)}
		>
			<ScrollAreaPrimitive.Thumb class={cn(defaultThumbBaseClasses, thumbXClass)} />
		</ScrollAreaPrimitive.Scrollbar>
	{/if}

	<ScrollAreaPrimitive.Corner class={cn(defaultCornerClasses, cornerClass)} />
</ScrollAreaPrimitive.Root>
