import Root from './ScrollArea.svelte';
import type { ScrollAreaRootProps as BitsScrollAreaRootProps } from 'bits-ui'; // Root props from bits-ui

// Our ScrollArea will accept RootProps from bits-ui, plus our own styling/control props
export type ScrollAreaProps = BitsScrollAreaRootProps & {
	orientation?: 'vertical' | 'horizontal' | 'both'; // To control which scrollbars are shown
	// Classes for individual parts if we want to allow deep styling via props
	viewportClass?: string;
	scrollbarXClass?: string;
	scrollbarYClass?: string;
	thumbXClass?: string;
	thumbYClass?: string;
	cornerClass?: string;
	// We can also add variants using tailwind-variants if needed
};

export {
	Root,
	// Alias
	Root as ScrollArea,
	type ScrollAreaProps
};
