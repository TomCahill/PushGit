// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import SelectHarness from "./SelectHarness.svelte";

describe("Select", () => {
  it("associates a visible label with the select via id/for", () => {
    const { getByLabelText } = render(SelectHarness, {
      props: { id: "provider", label: "Provider", value: "none" },
    });
    expect((getByLabelText("Provider") as HTMLSelectElement).value).toBe("none");
  });

  it("fires onchange with the new value", async () => {
    const onchange = vi.fn();
    const { getByLabelText } = render(SelectHarness, {
      props: { id: "provider", label: "Provider", value: "none", onchange },
    });
    const select = getByLabelText("Provider") as HTMLSelectElement;
    await fireEvent.change(select, { target: { value: "anthropic" } });
    expect(select.value).toBe("anthropic");
    expect(onchange).toHaveBeenCalledOnce();
  });
});
