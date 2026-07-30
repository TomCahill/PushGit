// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import ResizeHandle from "./ResizeHandle.svelte";
import { firePointer } from "./testPointerEvents";

describe("ResizeHandle", () => {
  it("reports the pointer's horizontal delta while dragging", () => {
    const onResize = vi.fn();
    const { getByRole } = render(ResizeHandle, { props: { onResize } });
    const handle = getByRole("separator");

    firePointer(handle, "pointerdown", { clientX: 100, pointerId: 1 });
    firePointer(handle, "pointermove", { clientX: 115, pointerId: 1 });
    firePointer(handle, "pointermove", { clientX: 108, pointerId: 1 });

    expect(onResize).toHaveBeenNthCalledWith(1, 15);
    expect(onResize).toHaveBeenNthCalledWith(2, -7);
  });

  it("stops reporting deltas once the pointer is released", () => {
    const onResize = vi.fn();
    const { getByRole } = render(ResizeHandle, { props: { onResize } });
    const handle = getByRole("separator");

    firePointer(handle, "pointerdown", { clientX: 100, pointerId: 1 });
    firePointer(handle, "pointerup", { clientX: 100, pointerId: 1 });
    firePointer(handle, "pointermove", { clientX: 150, pointerId: 1 });

    expect(onResize).not.toHaveBeenCalled();
  });

  it("nudges by a fixed step on arrow-left/right", async () => {
    const onResize = vi.fn();
    const { getByRole } = render(ResizeHandle, { props: { onResize } });
    const handle = getByRole("separator");

    await fireEvent.keyDown(handle, { key: "ArrowRight" });
    await fireEvent.keyDown(handle, { key: "ArrowLeft" });

    expect(onResize).toHaveBeenNthCalledWith(1, 20);
    expect(onResize).toHaveBeenNthCalledWith(2, -20);
  });

  it("reports the pointer's vertical delta while dragging when horizontal", () => {
    const onResize = vi.fn();
    const { getByRole } = render(ResizeHandle, {
      props: { onResize, orientation: "horizontal" },
    });
    const handle = getByRole("separator");

    firePointer(handle, "pointerdown", { clientX: 0, clientY: 100, pointerId: 1 });
    firePointer(handle, "pointermove", { clientX: 0, clientY: 115, pointerId: 1 });
    firePointer(handle, "pointermove", { clientX: 0, clientY: 108, pointerId: 1 });

    expect(onResize).toHaveBeenNthCalledWith(1, 15);
    expect(onResize).toHaveBeenNthCalledWith(2, -7);
  });

  it("nudges by a fixed step on arrow-up/down when horizontal", async () => {
    const onResize = vi.fn();
    const { getByRole } = render(ResizeHandle, {
      props: { onResize, orientation: "horizontal" },
    });
    const handle = getByRole("separator");

    await fireEvent.keyDown(handle, { key: "ArrowDown" });
    await fireEvent.keyDown(handle, { key: "ArrowUp" });

    expect(onResize).toHaveBeenNthCalledWith(1, 20);
    expect(onResize).toHaveBeenNthCalledWith(2, -20);
  });
});
