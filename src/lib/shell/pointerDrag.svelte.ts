// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Shared pointer-based drag-and-drop state machine behind RepoRail's repo/folder reordering
// and CommitGraph's ref-badge drag-to-merge/rebase/reset gesture — pointer events + manual
// `elementFromPoint` hit-testing rather than native HTML5 drag-and-drop, which doesn't
// reliably fire `dragover`/`drop` in the Tauri WebKitGTK webview. Only the mechanics
// (distance-gated drag detection, pointer capture, target-key-change caching, click
// suppression bookkeeping) are shared; each caller supplies its own source/target resolution
// and drop handling via `resolveSource`/`resolveTarget`/`onDrop`, since those differ per
// domain. `captureOn` and `onDrop`'s return value stay per-caller too, for reasons documented
// on each field below — they're real behavioral differences between the two call sites, not
// arbitrary ones ironed flat by this refactor.

export interface PointerDragOptions<TSource, TTarget> {
  /** Minimum pointer travel (px) before a pointerdown becomes a drag rather than a click. */
  threshold: number;
  /** "down" acquires pointer capture as soon as a valid source is found, before any drag is
   *  confirmed — right for a narrow, drag-only target (a ref badge) with no plain-click
   *  behavior of its own to protect. "move" defers capture until the threshold is actually
   *  crossed — right when the same element is *also* an ordinary click target, so capturing
   *  on every click would be wrong. */
  captureOn: "down" | "move";
  /** Resolves a pointerdown to a drag source, or `null` if this isn't a valid drag start. */
  resolveSource: (event: PointerEvent) => TSource | null;
  /** Resolves whatever's under (clientX, clientY) to a drop target, or `null`. */
  resolveTarget: (clientX: number, clientY: number) => TTarget | null;
  /** Cache key for a target, so reactive state only updates when the resolved target
   *  actually changes (avoids redundant re-renders on every pixel of pointer movement). */
  targetKey: (target: TTarget | null) => string;
  /** Called on every move once a real drag is in progress — for tracking extra live state
   *  (pointer position for a tooltip, modifier keys) beyond source/target alone. */
  onDragMove?: (event: PointerEvent) => void;
  /** Called once on release if a real drag happened (past the threshold), with whatever
   *  target was under the pointer at that point (possibly `null`). Return `true` to suppress
   *  the trailing synthetic `click` release generates, `false` to let it through so a
   *  drag-that-changed-nothing still selects/toggles normally — the two current callers
   *  differ here: RepoRail only suppresses when the drop actually did something, CommitGraph
   *  suppresses for any completed drag regardless of outcome, matching each one's own
   *  pre-existing behavior. */
  onDrop: (source: TSource, target: TTarget | null, event: PointerEvent) => boolean;
}

export interface PointerDrag<TSource, TTarget> {
  readonly source: TSource | null;
  readonly target: TTarget | null;
  handlePointerDown(event: PointerEvent): void;
  handlePointerMove(event: PointerEvent): void;
  handlePointerUp(event: PointerEvent): void;
  /** Call from the container's `onclick`, first thing: returns `true` (and clears the flag)
   *  if this click is the trailing one from a completed drag and should be swallowed. */
  consumeClickSuppression(): boolean;
}

export function createPointerDrag<TSource, TTarget>(
  options: PointerDragOptions<TSource, TTarget>,
): PointerDrag<TSource, TTarget> {
  // Plain (non-reactive) vars, mirroring `ResizeHandle.svelte`'s `last` — only read/written
  // synchronously within the handlers below, never need to trigger a re-render on their own.
  let pointerDownInfo: { x: number; y: number; source: TSource } | null = null;
  let suppressClick = false;

  let source = $state<TSource | null>(null);
  let target = $state<TTarget | null>(null);

  function handlePointerDown(event: PointerEvent) {
    // Left/primary button only — capturing on a right-button pointerdown has a real
    // cross-browser side effect beyond being semantically wrong: it suppresses the
    // `contextmenu` event that would otherwise fire on release.
    if (event.button !== 0) return;
    const candidate = options.resolveSource(event);
    if (!candidate) return;
    pointerDownInfo = { x: event.clientX, y: event.clientY, source: candidate };
    if (options.captureOn === "down") {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    }
  }

  function handlePointerMove(event: PointerEvent) {
    if (!pointerDownInfo) return;

    if (!source) {
      const distance = Math.hypot(
        event.clientX - pointerDownInfo.x,
        event.clientY - pointerDownInfo.y,
      );
      if (distance < options.threshold) return;
      source = pointerDownInfo.source;
      if (options.captureOn === "move") {
        (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      }
    }

    options.onDragMove?.(event);

    const candidate = options.resolveTarget(event.clientX, event.clientY);
    if (options.targetKey(candidate) !== options.targetKey(target)) {
      target = candidate;
    }
  }

  function handlePointerUp(event: PointerEvent) {
    // Safe to call even when this pointer never actually captured — releasing capture that
    // isn't held is a no-op per spec, not an error.
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);

    if (source && options.onDrop(source, target, event)) {
      suppressClick = true;
    }

    pointerDownInfo = null;
    source = null;
    target = null;
  }

  function consumeClickSuppression(): boolean {
    const wasSuppressed = suppressClick;
    suppressClick = false;
    return wasSuppressed;
  }

  return {
    get source() {
      return source;
    },
    get target() {
      return target;
    },
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    consumeClickSuppression,
  };
}
