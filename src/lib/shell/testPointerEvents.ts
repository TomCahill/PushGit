// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Test-only helper (not production code — see `$lib/git/testFixtures.ts` for the naming
// precedent) shared by any component test that simulates a pointer drag: jsdom has no
// `PointerEvent` constructor at all, and testing-library's `fireEvent.pointer*` helpers
// silently fall back to a plain `Event` (which drops non-standard init properties like
// `clientX`/`pointerId`) rather than erroring — so a `MouseEvent` with `pointerId` bolted on
// is used instead, since `MouseEvent`'s `clientX`/`clientY` are real, jsdom-supported init
// properties. Originally local to `ResizeHandle.svelte.test.ts`; promoted here once
// `CommitGraph.svelte.test.ts`'s drag-and-drop tests needed the same thing.
export function firePointer(
  element: Element,
  type: "pointerdown" | "pointermove" | "pointerup",
  {
    clientX,
    clientY = 0,
    pointerId,
    button = 0,
  }: { clientX: number; clientY?: number; pointerId: number; button?: number },
): void {
  const event = new MouseEvent(type, { clientX, clientY, button, bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerId", { value: pointerId });
  element.dispatchEvent(event);
}
