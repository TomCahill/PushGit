<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Test-only harness: `Button.svelte.test.ts` needs a `<Button>` instance with real
  // snippet content, which a plain `.test.ts` file can't express — same reasoning as
  // `PopoverHarness.svelte`.
  import Button from "./Button.svelte";

  let {
    label = "Click me",
    variant,
    type,
    disabled = false,
    onclick,
  }: {
    label?: string;
    variant?: "filled" | "tonal" | "outlined" | "text";
    type?: "button" | "submit";
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<Button {variant} {type} {disabled} {onclick}>{label}</Button>
