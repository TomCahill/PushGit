<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // M3 switch (track + thumb) drawn from a real `<input type="checkbox">` via `appearance:
  // none` and custom CSS, rather than a `role="switch"` div — keeps every existing
  // `getByRole("checkbox", { name: ... })` test (StagingPanel's "Skip hooks"/"Amend last
  // commit", this component's own callers) working unchanged.
  let {
    checked = $bindable(false),
    disabled = false,
    onchange,
    id,
    "aria-label": ariaLabel,
  }: {
    checked?: boolean;
    disabled?: boolean;
    onchange?: () => void;
    id?: string;
    "aria-label"?: string;
  } = $props();

  function handleClick(event: MouseEvent) {
    // jsdom (unlike real browsers) still toggles a disabled checkbox's `checked` state and
    // dispatches through to listeners on synthetic clicks — guard explicitly so `disabled` is
    // honored in tests, not just visually.
    if (disabled) event.preventDefault();
  }
</script>

<input
  {id}
  type="checkbox"
  class="switch"
  bind:checked
  {disabled}
  {onchange}
  onclick={handleClick}
  aria-label={ariaLabel}
/>

<style>
  .switch {
    appearance: none;
    -webkit-appearance: none;
    flex-shrink: 0;
    position: relative;
    width: 2.25rem;
    height: 1.25rem;
    margin: 0;
    border-radius: var(--radius-lg);
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    cursor: pointer;
    transition:
      background-color 0.15s ease,
      border-color 0.15s ease;
  }

  .switch::before {
    content: "";
    position: absolute;
    top: 50%;
    left: 0.2rem;
    width: 0.85rem;
    height: 0.85rem;
    border-radius: 50%;
    background: var(--border-strong);
    transform: translateY(-50%);
    transition:
      transform 0.15s ease,
      background-color 0.15s ease,
      width 0.15s ease,
      height 0.15s ease;
  }

  .switch:checked {
    background: var(--accent);
    border-color: var(--accent);
  }

  .switch:checked::before {
    background: var(--on-accent);
    width: 1rem;
    height: 1rem;
    transform: translate(0.85rem, -50%);
  }

  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .switch:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
