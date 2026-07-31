<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Same outlined-field chrome as `TextField`, wrapped around a native `<select>` rather
  // than a bespoke listbox — this app only ever has a couple of short option lists, so native
  // `<select>` keeps keyboard/screen-reader behavior for free instead of reimplementing it.
  import type { Snippet } from "svelte";

  let {
    id,
    label,
    value = $bindable(""),
    disabled = false,
    onchange,
    children,
  }: {
    id?: string;
    label?: string;
    value?: string;
    disabled?: boolean;
    onchange?: () => void;
    children: Snippet;
  } = $props();
</script>

<div class="field">
  {#if label}
    <label for={id}>{label}</label>
  {/if}
  <select {id} {disabled} bind:value {onchange}>
    {@render children()}
  </select>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  select {
    font: inherit;
    font-size: 0.85rem;
    padding: 0.55rem 0.75rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    transition: border-color 0.1s ease;
  }

  select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  select:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
