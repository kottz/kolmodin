import Root from './Input.svelte';
import type { HTMLInputAttributes } from 'svelte/elements';

// Props for our Input component will largely be HTMLInputAttributes
export type InputProps = HTMLInputAttributes;

// For Svelte 5, we don't need to explicitly define $$Events
// if we are relying on attribute-based event forwarding (e.g. onclick={...} in ...rest)
// and standard Svelte on:event syntax.

export {
	Root,
	type InputProps,
	// Export as Input for convenience
	Root as Input
};
