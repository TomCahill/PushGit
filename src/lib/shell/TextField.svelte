<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // M3 outlined text field. Two label modes, matching what call sites already need:
  // `label` renders a real floating `<label for>` (the usual case); omitting it in favor of
  // `ariaLabel` renders a compact field with only an `aria-label` and no visual label chrome,
  // for dense multi-field rows (the GitFlow setup grid) where a floating label per input
  // would be cramped — the accessible name still resolves the same way either way.
  let {
    id,
    label,
    ariaLabel,
    type = "text",
    inputmode,
    multiline = false,
    rows = 3,
    value = $bindable(""),
    placeholder,
    disabled = false,
    hint,
    oninput,
  }: {
    id?: string;
    label?: string;
    ariaLabel?: string;
    type?: "text" | "password";
    inputmode?: "text" | "numeric";
    multiline?: boolean;
    rows?: number;
    value?: string;
    placeholder?: string;
    disabled?: boolean;
    hint?: string;
    oninput?: () => void;
  } = $props();
</script>

<div class="field">
  {#if label}
    <label for={id}>{label}</label>
  {/if}
  {#if multiline}
    <textarea
      {id}
      {rows}
      {placeholder}
      {disabled}
      aria-label={label ? undefined : ariaLabel}
      bind:value
      {oninput}
    ></textarea>
  {:else}
    <input
      {id}
      {type}
      {inputmode}
      {placeholder}
      {disabled}
      aria-label={label ? undefined : ariaLabel}
      bind:value
      {oninput}
    />
  {/if}
  {#if hint}
    <p class="hint">{hint}</p>
  {/if}
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

  input,
  textarea {
    font: inherit;
    font-size: 0.85rem;
    padding: 0.55rem 0.75rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    transition: border-color 0.1s ease;
  }

  textarea {
    resize: none;
  }

  input:focus-visible,
  textarea:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  input:disabled,
  textarea:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
</style>
