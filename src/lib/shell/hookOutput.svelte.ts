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

import type { HookOutputLine } from "$lib/git/types";

export interface HookOutputSession {
  label: string;
  lines: HookOutputLine[];
  running: boolean;
}

export const hookOutputState: { session: HookOutputSession | null } = $state({ session: null });

export function startHookOutput(label: string): void {
  hookOutputState.session = { label, lines: [], running: true };
}

export function pushHookOutputLine(line: HookOutputLine): void {
  hookOutputState.session?.lines.push(line);
}

export function finishHookOutput(): void {
  if (hookOutputState.session) {
    hookOutputState.session.running = false;
  }
}

/** The modal's own close (×) button — hides the transcript without affecting whatever
 *  triggered it (there's nothing to cancel: push has its own cancel button elsewhere, and
 *  commit hooks aren't cancellable). */
export function closeHookOutput(): void {
  hookOutputState.session = null;
}
