<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Generic trigger+panel dropdown: click the trigger to open a panel anchored below it,
  // click outside or press Escape to close. Used by `ActionRail.svelte` to host
  // `BranchSidebar`/`TagsPanel`/`StashPanel` as toolbar dropdowns instead of always-on
  // sidebar sections — those components themselves don't know they're in a popover.
  // `children` stays mounted at all times (hidden via CSS, not `{#if}`) rather than being
  // created only while open: those three components report summary data back up (current
  // branch name, tag/stash counts) for the trigger button to show, which needs them loading
  // in the background before the user ever opens the dropdown, not only after.
  //
  // `portal`: ActionRail's four dropdowns live in the toolbar above the graph, not inside any
  // `overflow: auto` ancestor, so the default CSS-only anchoring (`position: absolute` relative
  // to the trigger) is enough. `RepoRail.svelte`'s Notifications button, though, sits inside
  // the left rail's `<aside class="repo-rail">`, which is itself a scroll container
  // (`+page.svelte`) — an absolutely-positioned panel wider than that rail gets clipped by its
  // `overflow: auto` instead of overlaying the rest of the app. `portal` opts a given instance
  // out of that by moving the panel to `document.body` and switching to `position: fixed`,
  // computed from the trigger's real bounding rect (the one case in this component that needs
  // JS measurement rather than pure CSS anchoring).
  import type { Snippet } from "svelte";

  let {
    trigger,
    children,
    align = "left",
    placement = "bottom",
    portal = false,
    class: klass = "",
  }: {
    trigger: Snippet<[{ open: boolean }]>;
    children: Snippet;
    align?: "left" | "right";
    placement?: "bottom" | "top";
    /** Escape a scrollable/clipping ancestor by rendering the panel into `document.body` with
     *  `position: fixed`, positioned from the trigger's live bounding rect. */
    portal?: boolean;
    class?: string;
  } = $props();

  const PANEL_GAP_PX = 6;

  let open = $state(false);
  let rootEl: HTMLDivElement | undefined = $state();
  let panelEl: HTMLDivElement | undefined = $state();
  let panelStyle = $state("");

  function computePanelPosition() {
    if (!rootEl) return;
    const rect = rootEl.getBoundingClientRect();
    const horizontal =
      align === "right"
        ? `right:${Math.round(window.innerWidth - rect.right)}px;`
        : `left:${Math.round(rect.left)}px;`;
    const vertical =
      placement === "top"
        ? `bottom:${Math.round(window.innerHeight - rect.top + PANEL_GAP_PX)}px;`
        : `top:${Math.round(rect.bottom + PANEL_GAP_PX)}px;`;
    panelStyle = horizontal + vertical;
  }

  function toggle() {
    open = !open;
    if (open && portal) computePanelPosition();
  }

  function handlePointerDown(event: PointerEvent) {
    if (!open) return;
    const target = event.target as Node;
    // `portal` moves `panelEl` out from under `rootEl` in the DOM, so a click inside the
    // portaled panel no longer registers as `rootEl.contains(target)` — check both instead of
    // just the root (a no-op in the non-portal case, where the panel is already inside root).
    const insideRoot = rootEl?.contains(target) ?? false;
    const insidePanel = panelEl?.contains(target) ?? false;
    if (!insideRoot && !insidePanel) {
      open = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") open = false;
  }

  function handleResize() {
    if (open && portal) computePanelPosition();
  }

  function portalToBody(node: HTMLElement, enabled: boolean) {
    if (enabled) document.body.appendChild(node);
    return {
      destroy() {
        node.parentNode?.removeChild(node);
      },
    };
  }
</script>

<svelte:window onpointerdown={handlePointerDown} onkeydown={handleKeydown} onresize={handleResize} />

<div class="popover {klass}" bind:this={rootEl}>
  <button type="button" class="trigger" class:open onclick={toggle} aria-expanded={open}>
    {@render trigger({ open })}
  </button>
  <div
    class="panel"
    class:align-right={align === "right"}
    class:placement-top={placement === "top"}
    class:hidden={!open}
    class:portal
    style={portal ? panelStyle : undefined}
    use:portalToBody={portal}
    bind:this={panelEl}
  >
    {@render children()}
  </div>
</div>

<style>
  .popover {
    position: relative;
    display: inline-flex;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition:
      background-color 0.1s ease,
      border-color 0.1s ease;
  }

  .trigger:hover {
    background: var(--surface-2);
  }

  .trigger.open {
    background: var(--surface-2);
    border-color: var(--accent);
    color: var(--accent);
  }

  .panel {
    position: absolute;
    z-index: 20;
    top: calc(100% + 0.4rem);
    left: 0;
    min-width: 20rem;
    max-width: 26rem;
    max-height: min(28rem, 70vh);
    overflow: auto;
    padding: 0.7rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
  }

  .panel.align-right {
    left: auto;
    right: 0;
  }

  .panel.placement-top {
    top: auto;
    bottom: calc(100% + 0.4rem);
  }

  .panel.hidden {
    display: none;
  }

  .panel.portal {
    position: fixed;
  }
</style>
