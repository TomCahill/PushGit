<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Small click-to-copy affordance reused wherever the app shows a value a user is likely
  // to want elsewhere (a commit SHA, a branch name, a file path) — none of those had any
  // copy mechanism at all before this. `stopPropagation` matters here: every one of this
  // button's current homes (a commit graph row, a branch-sidebar row, a diff file row) has
  // its own click handler for a different purpose (select the commit, nothing yet for a
  // branch row, select the file) that this button sits inside of.
  import Icon from "./Icon.svelte";

  let { text, label = "Copy" }: { text: string; label?: string } = $props();

  let copied = $state(false);
  let resetTimer: ReturnType<typeof setTimeout> | undefined;

  async function handleClick(event: MouseEvent) {
    event.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      clearTimeout(resetTimer);
      resetTimer = setTimeout(() => (copied = false), 1200);
    } catch {
      // Clipboard access can be denied (permissions, insecure context) — a silent no-op
      // beats an error banner for a purely cosmetic convenience action.
    }
  }
</script>

<button
  type="button"
  class="copy-button"
  class:copied
  onclick={handleClick}
  title={copied ? "Copied!" : label}
  aria-label={copied ? "Copied" : label}
>
  <Icon name={copied ? "check" : "copy"} size={12} />
</button>

<style>
  .copy-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0.1rem;
    color: var(--text-muted);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    opacity: 0.55;
    cursor: pointer;
    transition:
      opacity 0.1s ease,
      color 0.1s ease;
  }

  .copy-button:hover,
  .copy-button:focus-visible {
    opacity: 1;
    color: var(--accent);
  }

  .copy-button.copied {
    opacity: 1;
    color: var(--success);
  }
</style>
