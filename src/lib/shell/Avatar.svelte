<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Author identity pill: circular initials + name, shown wherever the app renders a commit
  // author (commit graph rows, blame history/gutter, commit-detail header). Color is a
  // deterministic hash of email/name — local-only by design, no Gravatar/network call. Font
  // size/color for the name deliberately aren't set here so each call site's own text styling
  // (size, weight, mono vs. sans) still applies, same as when the name was a plain text node.
  import { colorForIdentity } from "./avatarColor";

  let { name, email, size = 18 }: { name: string; email?: string; size?: number } = $props();

  const initials = $derived.by(() => {
    const words = name.trim().split(/\s+/).filter(Boolean);
    if (words.length === 0) return "";
    if (words.length === 1) return words[0][0].toUpperCase();
    return (words[0][0] + words[1][0]).toUpperCase();
  });

  const background = $derived(colorForIdentity((email?.trim().toLowerCase() || name).trim()));

  const OPEN_DELAY_MS = 300;
  const CARD_GAP_PX = 6;

  let cardVisible = $state(false);
  let cardStyle = $state("");
  let pillEl: HTMLSpanElement | undefined = $state();
  let openTimer: ReturnType<typeof setTimeout> | undefined;

  function showCard() {
    clearTimeout(openTimer);
    openTimer = setTimeout(() => {
      if (!pillEl) return;
      const rect = pillEl.getBoundingClientRect();
      cardStyle = `left:${Math.round(rect.left)}px;top:${Math.round(rect.bottom + CARD_GAP_PX)}px;`;
      cardVisible = true;
    }, OPEN_DELAY_MS);
  }

  function hideCard() {
    clearTimeout(openTimer);
    cardVisible = false;
  }

  function portalToBody(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.parentNode?.removeChild(node);
      },
    };
  }
</script>

<!-- Hover-only enrichment, not a control (nothing to click or focus) — same non-interactive
     hover pattern as this codebase's plain `title=` tooltips elsewhere. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<span class="author-pill" bind:this={pillEl} onmouseenter={showCard} onmouseleave={hideCard}>
  <span
    class="avatar"
    aria-hidden="true"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{size * 0.45}px"
    style:background={background}
  >
    {initials}
  </span>
  <span class="name">{name}</span>
</span>

{#if cardVisible}
  <div class="hover-card" style={cardStyle} use:portalToBody>
    <p class="hover-card-name">{name}</p>
    {#if email}
      <p class="hover-card-email">{email}</p>
    {/if}
  </div>
{/if}

<style>
  .author-pill {
    display: inline-flex;
    align-items: center;
    min-width: 0;
    max-width: 100%;
    gap: 0.35rem;
    padding: 0.1rem 0.55rem 0.1rem 0.1rem;
    border-radius: 999px;
    background: var(--surface-2);
  }

  .avatar {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: white;
    font-weight: 600;
    line-height: 1;
    user-select: none;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .hover-card {
    position: fixed;
    z-index: 90;
    min-width: 10rem;
    max-width: 18rem;
    padding: 0.5rem 0.65rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    pointer-events: none;
  }

  .hover-card-name {
    margin: 0;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .hover-card-email {
    margin: 0.15rem 0 0;
    font-size: 0.75rem;
    color: var(--text-muted);
    overflow-wrap: break-word;
  }
</style>
