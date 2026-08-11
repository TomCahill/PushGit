// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Live transcript for a running commit/push hook, rendered through the shared
// <HookOutputModal /> mounted once at the shell root (`+page.svelte`) — mirrors
// `confirmDialog.svelte.ts`'s imperative singleton pattern. `startHookOutput()` before the
// backend call, `pushHookOutputLine` as the `onHookOutput` callback, `finishHookOutput()` in
// the `finally` block; the transcript stays visible after `finish()` (only the next
// `start()` clears it) so the user can scroll back through what a hook printed, per the
// "keep it until the next operation" design decision. Generic on purpose — `label` is
// caller-supplied — so a future rebase/merge hook can reuse this without new plumbing.
//
// `visible` (settings.svelte.ts's `showHookOutputAlways`) controls whether `startHookOutput`
// opens the modal right away or keeps it hidden while still recording lines in the
// background — callers that support the "only on error" mode call `revealHookOutput()` from
// their own catch block when the operation actually fails.

import type { HookOutputLine } from "$lib/git/types";

export interface HookOutputSession {
  label: string;
  lines: HookOutputLine[];
  running: boolean;
  visible: boolean;
}

export const hookOutputState: { session: HookOutputSession | null } = $state({ session: null });

export function startHookOutput(label: string, visible = true): void {
  hookOutputState.session = { label, lines: [], running: true, visible };
}

export function pushHookOutputLine(line: HookOutputLine): void {
  hookOutputState.session?.lines.push(line);
}

export function finishHookOutput(): void {
  if (hookOutputState.session) {
    hookOutputState.session.running = false;
  }
}

/** Reveals a session that `startHookOutput` opened with `visible: false` — called from a
 *  caller's own catch block once an operation actually fails, so "only on error" mode still
 *  shows the transcript when there's something worth explaining. A no-op if already visible
 *  (or there's no session), so callers can call it unconditionally on failure. */
export function revealHookOutput(): void {
  if (hookOutputState.session) {
    hookOutputState.session.visible = true;
  }
}

/** The modal's own close (×) button — hides the transcript without affecting whatever
 *  triggered it (there's nothing to cancel: push has its own cancel button elsewhere, and
 *  commit hooks aren't cancellable). */
export function closeHookOutput(): void {
  hookOutputState.session = null;
}
