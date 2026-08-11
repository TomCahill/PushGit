// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { createPointerDrag } from "./pointerDrag.svelte";

const THRESHOLD = 5;

function pointerEvent(
  container: HTMLElement,
  overrides: Partial<{
    button: number;
    clientX: number;
    clientY: number;
    pointerId: number;
    altKey: boolean;
  }> = {},
): PointerEvent {
  return {
    button: 0,
    clientX: 0,
    clientY: 0,
    pointerId: 1,
    altKey: false,
    currentTarget: container,
    target: container,
    ...overrides,
  } as unknown as PointerEvent;
}

describe("createPointerDrag", () => {
  it("does not start a drag when movement stays below the threshold", () => {
    const container = document.createElement("div");
    const onDragMove = vi.fn();
    const drag = createPointerDrag<string, string>({
      threshold: THRESHOLD,
      captureOn: "down",
      resolveSource: () => "a",
      resolveTarget: () => "t",
      targetKey: (t) => t ?? "",
      onDragMove,
      onDrop: () => true,
    });

    drag.handlePointerDown(pointerEvent(container));
    drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD - 1 }));

    expect(drag.source).toBeNull();
    expect(onDragMove).not.toHaveBeenCalled();
  });

  it("starts a drag once movement crosses the threshold, resolving source and target", () => {
    const container = document.createElement("div");
    const drag = createPointerDrag<string, string>({
      threshold: THRESHOLD,
      captureOn: "down",
      resolveSource: () => "a",
      resolveTarget: () => "t",
      targetKey: (t) => t ?? "",
      onDrop: () => true,
    });

    drag.handlePointerDown(pointerEvent(container));
    drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));

    expect(drag.source).toBe("a");
    expect(drag.target).toBe("t");
  });

  it("ignores a non-left-button pointerdown entirely", () => {
    const container = document.createElement("div");
    const resolveSource = vi.fn(() => "a");
    const drag = createPointerDrag<string, string>({
      threshold: THRESHOLD,
      captureOn: "down",
      resolveSource,
      resolveTarget: () => "t",
      targetKey: (t) => t ?? "",
      onDrop: () => true,
    });

    drag.handlePointerDown(pointerEvent(container, { button: 2 }));
    expect(resolveSource).not.toHaveBeenCalled();

    drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD * 2 }));
    expect(drag.source).toBeNull();
  });

  it("does nothing on move when resolveSource found no valid drag start", () => {
    const container = document.createElement("div");
    const drag = createPointerDrag<string, string>({
      threshold: THRESHOLD,
      captureOn: "down",
      resolveSource: () => null,
      resolveTarget: () => "t",
      targetKey: (t) => t ?? "",
      onDrop: () => true,
    });

    drag.handlePointerDown(pointerEvent(container));
    drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD * 2 }));

    expect(drag.source).toBeNull();
  });

  it("ignores a pointermove with no preceding pointerdown", () => {
    const container = document.createElement("div");
    const resolveTarget = vi.fn(() => "t");
    const drag = createPointerDrag<string, string>({
      threshold: THRESHOLD,
      captureOn: "down",
      resolveSource: () => "a",
      resolveTarget,
      targetKey: (t) => t ?? "",
      onDrop: () => true,
    });

    drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD * 2 }));

    expect(resolveTarget).not.toHaveBeenCalled();
    expect(drag.source).toBeNull();
  });

  describe("capture timing", () => {
    it('acquires pointer capture immediately on pointerdown when captureOn is "down"', () => {
      const container = document.createElement("div");
      const capture = vi.spyOn(container, "setPointerCapture");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => null,
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));

      expect(capture).toHaveBeenCalledWith(1);
    });

    it('defers pointer capture until the drag threshold is crossed when captureOn is "move"', () => {
      const container = document.createElement("div");
      const capture = vi.spyOn(container, "setPointerCapture");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "move",
        resolveSource: () => "a",
        resolveTarget: () => null,
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      expect(capture).not.toHaveBeenCalled();

      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD - 1 }));
      expect(capture).not.toHaveBeenCalled();

      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      expect(capture).toHaveBeenCalledWith(1);
    });

    it("releases pointer capture on pointerup regardless of capture timing", () => {
      const container = document.createElement("div");
      const release = vi.spyOn(container, "releasePointerCapture");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "move",
        resolveSource: () => "a",
        resolveTarget: () => null,
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerUp(pointerEvent(container));

      expect(release).toHaveBeenCalledWith(1);
    });
  });

  describe("live move tracking", () => {
    it("calls onDragMove on the threshold-crossing move and every move after", () => {
      const container = document.createElement("div");
      const onDragMove = vi.fn();
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDragMove,
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      expect(onDragMove).toHaveBeenCalledTimes(1);

      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD + 1 }));
      expect(onDragMove).toHaveBeenCalledTimes(2);
    });
  });

  describe("target resolution", () => {
    it("updates the resolved target only when its key actually changes", () => {
      const container = document.createElement("div");
      let call = 0;
      const targets = ["x", "x", "y"];
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => targets[call++],
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      expect(drag.target).toBe("x");

      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD + 1 }));
      expect(drag.target).toBe("x");

      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD + 2 }));
      expect(drag.target).toBe("y");
    });
  });

  describe("drop and click suppression", () => {
    it("calls onDrop with the source, target, and the release event once a real drag completes", () => {
      const container = document.createElement("div");
      const onDrop = vi.fn(() => true);
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      const upEvent = pointerEvent(container, { clientX: THRESHOLD, clientY: 3 });
      drag.handlePointerUp(upEvent);

      expect(onDrop).toHaveBeenCalledWith("a", "t", upEvent);
    });

    it("does not call onDrop when the pointer never moved past the threshold", () => {
      const container = document.createElement("div");
      const onDrop = vi.fn(() => true);
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerUp(pointerEvent(container));

      expect(onDrop).not.toHaveBeenCalled();
    });

    it("calls onDrop with a null target when nothing resolves under the release point", () => {
      const container = document.createElement("div");
      const onDrop = vi.fn(() => false);
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => null,
        targetKey: (t) => t ?? "",
        onDrop,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      expect(onDrop).toHaveBeenCalledWith("a", null, expect.anything());
    });

    it("suppresses the next click only when onDrop returns true", () => {
      const container = document.createElement("div");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      expect(drag.consumeClickSuppression()).toBe(true);
    });

    it("does not suppress the next click when onDrop returns false (e.g. a no-op drop)", () => {
      const container = document.createElement("div");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => false,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      expect(drag.consumeClickSuppression()).toBe(false);
    });

    it("clears the suppression flag once consumed, so a second click is never suppressed", () => {
      const container = document.createElement("div");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      expect(drag.consumeClickSuppression()).toBe(true);
      expect(drag.consumeClickSuppression()).toBe(false);
    });

    it("does not suppress a plain click when no drag ever started", () => {
      const container = document.createElement("div");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerUp(pointerEvent(container));

      expect(drag.consumeClickSuppression()).toBe(false);
    });
  });

  describe("state reset", () => {
    it("resets source and target after release so the UI stops showing a drag in progress", () => {
      const container = document.createElement("div");
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => "a",
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      expect(drag.source).toBeNull();
      expect(drag.target).toBeNull();
    });

    it("leaves no stale state behind for the next drag", () => {
      const container = document.createElement("div");
      let call = 0;
      const sources = ["a", "b"];
      const drag = createPointerDrag<string, string>({
        threshold: THRESHOLD,
        captureOn: "down",
        resolveSource: () => sources[call++],
        resolveTarget: () => "t",
        targetKey: (t) => t ?? "",
        onDrop: () => true,
      });

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));
      drag.handlePointerUp(pointerEvent(container, { clientX: THRESHOLD }));

      drag.handlePointerDown(pointerEvent(container));
      drag.handlePointerMove(pointerEvent(container, { clientX: THRESHOLD }));

      expect(drag.source).toBe("b");
    });
  });
});
