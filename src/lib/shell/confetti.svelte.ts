// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Confetti burst on a successful push, following the same imperative-singleton pattern as
// `confirmDialog.svelte.ts`/`toast.svelte.ts`: call `burstConfetti(rect)` from anywhere, rendered
// by the one `<Confetti />` mounted at the shell root (`+page.svelte`). No-ops under reduce-motion
// (`.private/feature/push-confetti/PLAN.md`) — confetti is purely decorative with no information
// content, so there's nothing lost by skipping it outright rather than showing a reduced variant.

import { prefersReducedMotion } from "./motion";

export type ConfettiBurst = {
  id: number;
  x: number;
  y: number;
};

export const confettiState: { bursts: ConfettiBurst[] } = $state({ bursts: [] });

let nextId = 0;

/** Queues a burst centered on `origin` (typically a button's `getBoundingClientRect()`). */
export function burstConfetti(origin: DOMRect): void {
  if (prefersReducedMotion()) return;
  confettiState.bursts = [
    ...confettiState.bursts,
    { id: ++nextId, x: origin.left + origin.width / 2, y: origin.top + origin.height / 2 },
  ];
}

/** Called by `Confetti.svelte` once a burst's particles have all finished animating — not a
 *  fixed timer like toast auto-dismiss, since particle lifetime varies with initial velocity. */
export function endBurst(id: number): void {
  confettiState.bursts = confettiState.bursts.filter((burst) => burst.id !== id);
}
