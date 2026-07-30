// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach } from "vitest";
import { cleanup } from "@testing-library/svelte";
import { clearMocks } from "@tauri-apps/api/mocks";

// jsdom doesn't implement ResizeObserver; Svelte's `bind:clientHeight` (used by
// CommitGraph's virtualization) relies on it, so components using it need at least a
// no-op stand-in to mount under Vitest.
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as unknown as typeof ResizeObserver;

// jsdom doesn't implement the Clipboard API at all; CopyButton.svelte calls
// navigator.clipboard.writeText, so it needs at least a stand-in to call without
// throwing. Tests asserting what was actually copied use
// `vi.spyOn(navigator.clipboard, "writeText")` to override this per test.
Object.defineProperty(globalThis.navigator, "clipboard", {
  value: { writeText: async () => {} },
  configurable: true,
});

// jsdom doesn't implement pointer capture either; ResizeHandle.svelte calls
// set/releasePointerCapture on drag start/end, so they need at least a no-op stand-in.
if (!HTMLElement.prototype.setPointerCapture) {
  HTMLElement.prototype.setPointerCapture = () => {};
  HTMLElement.prototype.releasePointerCapture = () => {};
}

// jsdom has no layout engine, so it doesn't implement elementFromPoint at all (not even a
// stub — unlike some other geometry APIs). CommitGraph.svelte's drag-and-drop uses it for
// live drop-target hit-testing on every pointermove, so it needs at least a stand-in to spy
// on. Tests that need a specific "what's under the cursor" answer use
// `vi.spyOn(document, "elementFromPoint").mockReturnValue(...)` to override this per test.
if (!document.elementFromPoint) {
  document.elementFromPoint = () => null;
}

// jsdom doesn't implement the Web Animations API; Svelte's `transition:`/`animate:` directives
// (added to `ConfirmDialog`/`ContextMenu`/`CommandPalette`/`Toast` by the reduce-motion pass,
// `.private/feature/reduce-motion-setting/PLAN.md`'s Phase C) call `element.animate()`/
// `element.getAnimations()` directly, so any component using them needs at least a stand-in to
// mount and unmount under Vitest. `onfinish` fires on a microtask rather than after `duration` —
// existing tests only await a promise (`fireEvent`'s return value, an explicit
// `Promise.resolve()`) rather than real elapsed time, and (unlike `setTimeout`) a microtask
// isn't affected by a test's `vi.useFakeTimers()`, so this resolves under either timer mode
// without the suite needing to wait out real transition durations.
if (!Element.prototype.animate) {
  Element.prototype.animate = function (): Animation {
    const fake = {
      currentTime: 0,
      playState: "running",
      effect: null,
      onfinish: null as (() => void) | null,
      cancel(): void {
        fake.playState = "idle";
      },
      finish(): void {
        fake.playState = "finished";
      },
    };
    queueMicrotask(() => {
      fake.playState = "finished";
      fake.onfinish?.();
    });
    return fake as unknown as Animation;
  };
}
if (!Element.prototype.getAnimations) {
  Element.prototype.getAnimations = () => [];
}

// Every component test that calls mockIPC must have
// its mock cleared afterward so IPC handlers don't leak between tests.
afterEach(() => {
  cleanup();
  clearMocks();
  localStorage.clear();
});
