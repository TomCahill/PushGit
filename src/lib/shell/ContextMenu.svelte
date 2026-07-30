<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Generic right-click menu, rendered wherever `contextMenu.svelte.ts`'s singleton
  // `menuState.request` currently holds one — mounted once at the shell root, same pattern as
  // `ConfirmDialog`. Unlike `Popover.svelte` (anchored under a trigger button), this is
  // positioned at an arbitrary viewport point (the cursor) and clamped to stay on-screen.
  import { scale } from "svelte/transition";
  import { menuState, closeContextMenu, type ContextMenuAction } from "./contextMenu.svelte";
  import { motionFast } from "./motion";

  let menuEl: HTMLUListElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  // Runs once against the raw click point immediately (so something paints right away), then
  // again once `menuEl` is bound and has real dimensions to clamp against — re-reads `menuEl`
  // on every run so the second pass is picked up as soon as the element mounts.
  $effect(() => {
    const request = menuState.request;
    if (!request) return;
    if (!menuEl) {
      left = request.x;
      top = request.y;
      return;
    }
    const margin = 4;
    left = Math.max(margin, Math.min(request.x, window.innerWidth - menuEl.offsetWidth - margin));
    top = Math.max(margin, Math.min(request.y, window.innerHeight - menuEl.offsetHeight - margin));
  });

  // Capture phase: `.graph-scroll`'s internal scrolling doesn't bubble to `window`, and a
  // `position: fixed` menu would otherwise visually detach from whatever it was anchored to.
  $effect(() => {
    if (!menuState.request) return;
    const handleScroll = () => closeContextMenu();
    window.addEventListener("scroll", handleScroll, true);
    return () => window.removeEventListener("scroll", handleScroll, true);
  });

  function handlePointerDown(event: PointerEvent) {
    if (menuState.request && menuEl && !menuEl.contains(event.target as Node)) {
      closeContextMenu();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (menuState.request && event.key === "Escape") {
      event.preventDefault();
      closeContextMenu();
    }
  }

  function handleItemClick(item: ContextMenuAction) {
    if (item.disabled) return;
    item.onSelect();
    closeContextMenu();
  }
</script>

<svelte:window onpointerdown={handlePointerDown} onkeydown={handleKeydown} />

{#if menuState.request}
  {@const request = menuState.request}
  <ul
    class="context-menu"
    role="menu"
    bind:this={menuEl}
    style:left="{left}px"
    style:top="{top}px"
    transition:scale={{ duration: motionFast(), start: 0.94 }}
  >
    {#each request.items as item, i (i)}
      {#if "separator" in item}
        <li class="separator" role="separator"></li>
      {:else}
        <li>
          <button
            type="button"
            role="menuitem"
            class="item"
            class:danger={item.danger}
            disabled={item.disabled}
            onclick={() => handleItemClick(item)}
          >
            {item.label}
          </button>
        </li>
      {/if}
    {/each}
  </ul>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 90;
    min-width: 12rem;
    margin: 0;
    padding: 0.3rem;
    list-style: none;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }

  .separator {
    height: 1px;
    margin: 0.3rem 0.2rem;
    background: var(--border);
  }

  .item {
    display: block;
    width: 100%;
    padding: 0.35rem 0.5rem;
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .item:not(:disabled):hover {
    background: var(--accent-bg);
  }

  .item:disabled {
    color: var(--text-muted);
    cursor: default;
  }

  .item.danger {
    color: var(--danger);
  }

  .item.danger:not(:disabled):hover {
    background: var(--danger-bg);
  }
</style>
