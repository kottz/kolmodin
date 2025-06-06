<script lang="ts">
	import { cn } from '$lib/utils/cn';

	interface Props {
		variant?: 'default' | 'secondary' | 'destructive' | 'outline';
		class?: string;
	}

	let { variant = 'default', class: className, children, ...restProps }: Props = $props();

	const variants = {
		default: 'bg-primary text-primary-foreground shadow hover:bg-primary/80',
		secondary: 'bg-secondary text-secondary-foreground hover:bg-secondary/80',
		destructive: 'bg-destructive text-destructive-foreground shadow hover:bg-destructive/80',
		outline:
			'text-foreground border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground'
	};
</script>

<div
	class={cn(
		'focus:ring-ring inline-flex items-center rounded-md border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:ring-2 focus:ring-offset-2 focus:outline-none',
		variants[variant],
		className
	)}
	{...restProps}
>
	{@render children?.()}
</div>
