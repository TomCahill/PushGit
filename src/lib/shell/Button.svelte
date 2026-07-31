<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Shared M3 button (filled/tonal/outlined/text) so every panel draws from one
  // implementation instead of each pasting its own copy of the same `--btn-*`-token CSS
  // (StagingPanel, RemotePanel, ConfirmDialog, AboutDialog, InteractiveRebaseEditor,
  // CherryPickRangePicker, RepoRail all did this independently before this component existed).
  import type { Snippet } from "svelte";

  let {
    variant = "filled",
    type = "button",
    disabled = false,
    title,
    onclick,
    children,
    ref = $bindable(null),
  }: {
    variant?: "filled" | "tonal" | "outlined" | "text";
    type?: "button" | "submit";
    disabled?: boolean;
    title?: string;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
    ref?: HTMLButtonElement | null;
  } = $props();

  function handleClick(event: MouseEvent) {
    // jsdom (unlike real browsers) still dispatches synthetic clicks through to a disabled
    // button's listener — guard explicitly so `disabled` is honored in tests, not just visually.
    if (disabled) return;
    onclick?.(event);
  }
</script>

<button {type} {title} class="btn btn-{variant}" {disabled} onclick={handleClick} bind:this={ref}>
  {@render children()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    font: inherit;
    font-size: 0.85rem;
    font-weight: 500;
    padding: 0.5rem 1.1rem;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition:
      opacity 0.1s ease,
      background-color 0.1s ease;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .btn-filled {
    color: var(--btn-filled-fg);
    background: var(--btn-filled-bg);
    border: none;
  }

  .btn-filled:not(:disabled):hover {
    opacity: 0.9;
  }

  .btn-tonal {
    color: var(--btn-tonal-fg);
    background: var(--btn-tonal-bg);
    border: none;
  }

  .btn-tonal:not(:disabled):hover {
    opacity: 0.85;
  }

  .btn-outlined {
    color: var(--btn-outlined-fg);
    background: transparent;
    border: 1px solid var(--btn-outlined-border);
  }

  .btn-outlined:not(:disabled):hover {
    background: var(--accent-bg);
  }

  .btn-text {
    color: var(--accent);
    background: transparent;
    border: none;
    padding: 0.4rem 0.6rem;
  }

  .btn-text:not(:disabled):hover {
    background: var(--accent-bg);
  }
</style>
