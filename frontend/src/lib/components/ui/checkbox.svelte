<script lang="ts">
	import { Checkbox as CheckboxPrimitive } from 'bits-ui';
	import Check from 'lucide-svelte/icons/check';
	import Minus from 'lucide-svelte/icons/minus';
	import { cn } from '$lib/utils/cn';

	interface Props {
		id?: string;
		checked?: boolean;
		onCheckedChange?: (checked: boolean) => void;
		disabled?: boolean;
		indeterminate?: boolean;
		class?: string;
	}

	let {
		id,
		checked = $bindable(false),
		onCheckedChange,
		disabled = false,
		indeterminate = false,
		class: className = ''
	}: Props = $props();

	function handleChange(checkedState: boolean) {
		checked = checkedState;
		onCheckedChange?.(checkedState);
	}
</script>

<CheckboxPrimitive.Root
	{id}
	class={cn(
		'peer border-primary ring-offset-background focus-visible:ring-ring data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground h-4 w-4 shrink-0 rounded-sm border focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50',
		className
	)}
	bind:checked
	onCheckedChange={handleChange}
	{disabled}
	{indeterminate}
>
	{#snippet children({ checked: isChecked, indeterminate: isIndeterminate })}
		{#if isChecked}
			<Check class="h-4 w-4" />
		{:else if isIndeterminate}
			<Minus class="h-4 w-4" />
		{/if}
	{/snippet}
</CheckboxPrimitive.Root>
