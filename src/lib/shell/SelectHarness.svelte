<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Test-only harness: `Select.svelte.test.ts` needs a `<Select>` instance with real
  // `<option>` snippet content, which a plain `.test.ts` file can't express — same reasoning
  // as `ButtonHarness.svelte`/`PopoverHarness.svelte`.
  import Select from "./Select.svelte";

  let {
    id,
    label,
    value = $bindable(""),
    onchange,
  }: {
    id?: string;
    label?: string;
    value?: string;
    onchange?: () => void;
  } = $props();
</script>

<Select {id} {label} bind:value {onchange}>
  <option value="none">None</option>
  <option value="anthropic">Anthropic</option>
</Select>
