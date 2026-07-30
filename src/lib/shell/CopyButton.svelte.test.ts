// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import CopyButton from "./CopyButton.svelte";

describe("CopyButton", () => {
  it("copies the given text to the clipboard and shows a brief confirmation", async () => {
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();

    const { getByRole } = render(CopyButton, { props: { text: "abc123", label: "Copy SHA" } });

    const button = getByRole("button", { name: "Copy SHA" });
    await fireEvent.click(button);

    expect(writeText).toHaveBeenCalledWith("abc123");
    await waitFor(() => expect(getByRole("button", { name: "Copied" })).toBeTruthy());
  });
});
