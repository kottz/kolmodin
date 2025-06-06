<script lang="ts">
	import { Checkbox, Label } from 'bits-ui';
	import Check from 'lucide-svelte/icons/check';
	import Minus from 'lucide-svelte/icons/minus';

	interface Props {
		id: string;
		checked?: boolean;
		onCheckedChange?: (checked: boolean) => void;
		disabled?: boolean;
		indeterminate?: boolean;
		label: string;
		class?: string;
	}

	let {
		id,
		checked = $bindable(false),
		onCheckedChange,
		disabled = false,
		indeterminate = false,
		label,
		class: className = ''
	}: Props = $props();

	function handleChange(checkedState: boolean) {
		checked = checkedState;
		onCheckedChange?.(checkedState);
	}
</script>

<div class="flex items-center space-x-3 {className}">
	<Checkbox.Root
		{id}
		aria-labelledby="{id}-label"
		class="border-muted bg-foreground data-[state=unchecked]:border-border-input data-[state=unchecked]:bg-background data-[state=unchecked]:hover:border-dark-40 peer inline-flex size-[25px] items-center justify-center rounded-md border transition-all duration-150 ease-in-out active:scale-[0.98]"
		bind:checked
		onCheckedChange={handleChange}
		{disabled}
		{indeterminate}
	>
		{#snippet children({ checked: isChecked, indeterminate: isIndeterminate })}
			<div class="text-background inline-flex items-center justify-center">
				{#if isIndeterminate}
					<Minus class="size-4" />
				{:else if isChecked}
					<Check class="size-4" />
				{/if}
			</div>
		{/snippet}
	</Checkbox.Root>
	<Label.Root
		id="{id}-label"
		for={id}
		class="text-sm leading-none font-medium peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
	>
		{label}
	</Label.Root>
</div>
