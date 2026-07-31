// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import Switch from "./Switch.svelte";

describe("Switch", () => {
  it("renders as a real checkbox with the given accessible name", () => {
    const { getByRole } = render(Switch, {
      props: { "aria-label": "Skip hooks", checked: false },
    });
    const input = getByRole("checkbox", { name: "Skip hooks" }) as HTMLInputElement;
    expect(input.type).toBe("checkbox");
    expect(input.checked).toBe(false);
  });

  it("toggles and fires onchange on click", async () => {
    const onchange = vi.fn();
    const { getByRole } = render(Switch, {
      props: { "aria-label": "Reduce motion", checked: false, onchange },
    });
    const input = getByRole("checkbox", { name: "Reduce motion" }) as HTMLInputElement;
    await fireEvent.click(input);
    expect(input.checked).toBe(true);
    expect(onchange).toHaveBeenCalledOnce();
  });

  it("does not toggle when disabled", async () => {
    const { getByRole } = render(Switch, {
      props: { "aria-label": "Skip hooks", checked: false, disabled: true },
    });
    const input = getByRole("checkbox", { name: "Skip hooks" }) as HTMLInputElement;
    await fireEvent.click(input);
    expect(input.checked).toBe(false);
  });
});
