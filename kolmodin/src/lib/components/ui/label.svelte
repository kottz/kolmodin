<script lang="ts">
	import { Checkbox, Label } from 'bits-ui';
	//import Check from "phosphor-svelte/lib/Check";
	//import Minus from "phosphor-svelte/lib/Minus";
</script>

<div class="flex items-center space-x-3">
	<Checkbox.Root
		id="terms"
		aria-labelledby="terms-label"
		class="border-muted bg-foreground data-[state=unchecked]:border-border-input data-[state=unchecked]:bg-background data-[state=unchecked]:hover:border-dark-40 peer inline-flex size-[25px] items-center justify-center rounded-md border transition-all duration-150 ease-in-out active:scale-[0.98]"
		name="hello"
	>
		{#snippet children({ checked, indeterminate })}
			<div class="text-background inline-flex items-center justify-center">
				{#if indeterminate}{:else if checked}{/if}
			</div>
		{/snippet}
	</Checkbox.Root>
	<Label.Root
		id="terms-label"
		for="terms"
		class="text-sm leading-none font-medium peer-disabled:cursor-not-allowed peer-disabled:opacity-70"
	>
		Accept terms and conditions
	</Label.Root>
</div>
