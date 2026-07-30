<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Drag divider between two grid columns (orientation "vertical", the default) or two
  // stacked flex rows (orientation "horizontal") — e.g. the 3-pane shell's columns, or
  // `StagingPanel.svelte`'s stacked Changes/Staged/Diff/commit sections. Reports the
  // pointer's delta along the resize axis each move; clamping and persisting the resulting
  // size is the caller's job (`paneWidths.svelte.ts`, `sectionHeights.svelte.ts`), not this
  // generic handle's. Pointer capture (rather than window-level move/up listeners) keeps drag
  // events flowing to this element even when the pointer briefly leaves it mid-drag, without
  // any manual listener add/remove lifecycle.
  let {
    onResize,
    orientation = "vertical",
  }: { onResize: (delta: number) => void; orientation?: "vertical" | "horizontal" } = $props();

  const KEYBOARD_STEP = 20;

  let dragging = $state(false);
  let last = 0;

  function pointerPos(event: PointerEvent): number {
    return orientation === "vertical" ? event.clientX : event.clientY;
  }

  function handlePointerDown(event: PointerEvent) {
    dragging = true;
    last = pointerPos(event);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent) {
    if (!dragging) return;
    const pos = pointerPos(event);
    const delta = pos - last;
    last = pos;
    if (delta !== 0) onResize(delta);
  }

  function handlePointerUp(event: PointerEvent) {
    dragging = false;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  function handleKeydown(event: KeyboardEvent) {
    const [decKey, incKey] =
      orientation === "vertical" ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"];
    if (event.key === decKey) {
      event.preventDefault();
      onResize(-KEYBOARD_STEP);
    } else if (event.key === incKey) {
      event.preventDefault();
      onResize(KEYBOARD_STEP);
    }
  }
</script>

<!-- WAI-ARIA APG "Window Splitter" pattern: a focusable, keyboard-operable separator is
     the correct role here, but Svelte's a11y linter doesn't special-case `role="separator"`
     as inherently interactive. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="resize-handle"
  class:horizontal={orientation === "horizontal"}
  class:dragging
  role="separator"
  aria-orientation={orientation === "vertical" ? "vertical" : "horizontal"}
  tabindex="0"
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onkeydown={handleKeydown}
></div>

<style>
  .resize-handle {
    flex-shrink: 0;
    width: 5px;
    cursor: col-resize;
    background: transparent;
    transition: background-color 0.1s ease;
  }

  .resize-handle.horizontal {
    width: auto;
    height: 5px;
    cursor: row-resize;
  }

  .resize-handle:hover,
  .resize-handle.dragging {
    background: var(--accent);
  }

  .resize-handle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
</style>
