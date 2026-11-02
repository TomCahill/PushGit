// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render, within } from "@testing-library/svelte";
import { tick } from "svelte";
import ConfirmDialog from "./ConfirmDialog.svelte";
import { confirmAsync } from "./confirmDialog.svelte";

async function settled<T>(promise: Promise<T>): Promise<T | "pending"> {
  return Promise.race([promise, tick().then(() => "pending" as const)]);
}

describe("ConfirmDialog", () => {
  it("shows the title and a named confirm button", async () => {
    const { findByRole } = render(ConfirmDialog);

    const result = confirmAsync("It goes away.", {
      title: "Drop stash?",
      confirmLabel: "Drop stash",
    });
    const dialog = within(await findByRole("alertdialog"));
    expect(dialog.getByRole("heading", { name: "Drop stash?" })).toBeTruthy();
    await fireEvent.click(dialog.getByRole("button", { name: "Drop stash" }));

    expect(await result).toBe(true);
  });

  it("focuses Cancel and ignores a stray Enter for a danger confirm", async () => {
    const { findByRole } = render(ConfirmDialog);

    const result = confirmAsync("Gone for good.", { confirmLabel: "Delete", danger: true });
    const dialog = within(await findByRole("alertdialog"));
    await tick();
    expect(document.activeElement).toBe(dialog.getByRole("button", { name: "Cancel" }));

    await fireEvent.keyDown(window, { key: "Enter" });
    expect(await settled(result)).toBe("pending");

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(await result).toBe(false);
  });

  it("still confirms on Enter for a non-danger confirm", async () => {
    const { findByRole } = render(ConfirmDialog);

    const result = confirmAsync("Replace it?");
    const dialog = within(await findByRole("alertdialog"));
    expect(dialog.getByRole("button", { name: "OK" })).toBeTruthy();
    await fireEvent.keyDown(window, { key: "Enter" });

    expect(await result).toBe(true);
  });
});
