// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import ActionRail from "./ActionRail.svelte";

// repoPath: "" makes every child panel (BranchSidebar/TagsPanel/StashPanel/RemotePanel) skip
// its own data-fetching, matching the "does nothing when no repo is open" convention every
// sibling panel test already relies on — lets this test exercise just the search box without
// mocking every command those panels would otherwise call.
describe("ActionRail search box", () => {
  it("fires onSearchChange on every keystroke", async () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });
    const onSearchChange = vi.fn();

    const { getByLabelText } = render(ActionRail, {
      props: {
        repoPath: "",
        refreshKey: 0,
        onChanged: vi.fn(),
        onConflicts: vi.fn(),
        onApplied: vi.fn(),
        onSearchChange,
      },
    });

    const input = getByLabelText("Search commit graph") as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "fix bug" } });

    expect(onSearchChange).toHaveBeenCalledWith("fix bug");
    expect(input.value).toBe("fix bug");
  });
});
