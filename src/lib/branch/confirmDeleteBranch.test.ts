// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render, within } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import { confirmDeleteBranch } from "./confirmDeleteBranch";

describe("confirmDeleteBranch", () => {
  it("warns how many commits an unmerged branch would orphan", async () => {
    mockIPC((cmd, args) => {
      expect(cmd).toBe("branch_unmerged_count");
      expect(args).toMatchObject({ repoPath: "/repo", name: "spike" });
      return 3;
    });
    const { findByRole } = render(ConfirmDialog);

    const result = confirmDeleteBranch("/repo", "spike");
    const dialog = within(await findByRole("alertdialog"));
    expect(dialog.getByRole("heading", { name: "Delete unmerged branch?" })).toBeTruthy();
    expect(dialog.getByText(/has 3 commits that aren't on any other branch/)).toBeTruthy();
    await fireEvent.click(dialog.getByRole("button", { name: "Delete anyway" }));

    expect(await result).toBe(true);
  });

  it("falls back to the plain prompt when the count fails", async () => {
    mockIPC(() => {
      throw "Reference not found";
    });
    const { findByRole } = render(ConfirmDialog);

    const result = confirmDeleteBranch("/repo", "spike");
    const dialog = within(await findByRole("alertdialog"));
    expect(dialog.getByRole("heading", { name: "Delete branch?" })).toBeTruthy();
    await fireEvent.click(dialog.getByRole("button", { name: "Cancel" }));

    expect(await result).toBe(false);
  });
});
