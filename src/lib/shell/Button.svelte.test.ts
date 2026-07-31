// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import ButtonHarness from "./ButtonHarness.svelte";

describe("Button", () => {
  it("renders a real button with the given label and variant class", () => {
    const { getByRole } = render(ButtonHarness, { props: { label: "Save", variant: "tonal" } });
    const button = getByRole("button", { name: "Save" });
    expect(button.className).toContain("btn-tonal");
  });

  it("fires onclick when enabled", async () => {
    const onclick = vi.fn();
    const { getByRole } = render(ButtonHarness, { props: { label: "Go", onclick } });
    await fireEvent.click(getByRole("button", { name: "Go" }));
    expect(onclick).toHaveBeenCalledOnce();
  });

  it("does not fire onclick when disabled", async () => {
    const onclick = vi.fn();
    const { getByRole } = render(ButtonHarness, {
      props: { label: "Go", onclick, disabled: true },
    });
    await fireEvent.click(getByRole("button", { name: "Go" }));
    expect(onclick).not.toHaveBeenCalled();
  });

  it("defaults to the filled variant and type=button", () => {
    const { getByRole } = render(ButtonHarness);
    const button = getByRole("button") as HTMLButtonElement;
    expect(button.className).toContain("btn-filled");
    expect(button.type).toBe("button");
  });
});
