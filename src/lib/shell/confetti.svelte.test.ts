// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { burstConfetti, confettiState, endBurst } from "./confetti.svelte";

function rect(x: number, y: number, width: number, height: number): DOMRect {
  return { x, y, left: x, top: y, width, height, right: x + width, bottom: y + height } as DOMRect;
}

describe("confetti", () => {
  beforeEach(() => {
    confettiState.bursts = [];
  });

  afterEach(() => {
    document.documentElement.removeAttribute("data-reduce-motion");
  });

  it("queues a burst centered on the given origin rect", () => {
    burstConfetti(rect(100, 200, 40, 20));

    expect(confettiState.bursts).toHaveLength(1);
    expect(confettiState.bursts[0]).toMatchObject({ x: 120, y: 210 });
  });

  it("is a no-op under reduce-motion", () => {
    document.documentElement.setAttribute("data-reduce-motion", "true");

    burstConfetti(rect(0, 0, 10, 10));

    expect(confettiState.bursts).toEqual([]);
  });

  it("assigns each burst a distinct, increasing id", () => {
    burstConfetti(rect(0, 0, 10, 10));
    burstConfetti(rect(0, 0, 10, 10));

    const [first, second] = confettiState.bursts;
    expect(second.id).toBeGreaterThan(first.id);
  });

  it("endBurst removes only the matching entry from the shared state", () => {
    burstConfetti(rect(0, 0, 10, 10));
    burstConfetti(rect(0, 0, 10, 10));
    const [first, second] = confettiState.bursts;

    endBurst(first.id);

    expect(confettiState.bursts).toEqual([second]);
  });
});
