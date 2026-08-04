// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import CommitDiffView from "./CommitDiffView.svelte";
import { makeFileDiff } from "$lib/git/testFixtures";

describe("CommitDiffView", () => {
  it("lists the changed files", async () => {
    const files = [
      makeFileDiff({ newPath: "a.txt", status: "modified" }),
      makeFileDiff({ newPath: "b.txt", status: "added" }),
    ];

    const { findByText } = render(CommitDiffView, { props: { files } });

    expect(await findByText("a.txt")).toBeTruthy();
    expect(await findByText("b.txt")).toBeTruthy();
  });

  it("shows the old and new path for a renamed file", async () => {
    const files = [makeFileDiff({ oldPath: "old.txt", newPath: "new.txt", status: "renamed" })];

    const { findByText } = render(CommitDiffView, { props: { files } });

    expect(await findByText("old.txt → new.txt")).toBeTruthy();
  });

  it("shows insertion/deletion counts per file and as a header total", async () => {
    const files = [
      makeFileDiff({ newPath: "a.txt", insertions: 5, deletions: 2 }),
      makeFileDiff({ newPath: "b.txt", insertions: 1, deletions: 1 }),
    ];

    const { findByText } = render(CommitDiffView, { props: { files } });

    expect(await findByText("2 file(s) changed")).toBeTruthy();
    expect(await findByText("+5")).toBeTruthy();
    expect(await findByText("-2")).toBeTruthy();
    expect(await findByText("+1")).toBeTruthy();
    expect(await findByText("-1")).toBeTruthy();
    expect(await findByText("+6")).toBeTruthy();
    expect(await findByText("-3")).toBeTruthy();
  });

  it("copies a file's path without selecting it", async () => {
    const files = [makeFileDiff({ newPath: "a.txt" })];
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();

    const { container, findByRole } = render(CommitDiffView, { props: { files } });

    await fireEvent.click(await findByRole("button", { name: "Copy path a.txt" }));

    expect(writeText).toHaveBeenCalledWith("a.txt");
    expect(container.querySelector("li.selected")).toBeNull();
  });

  it("marks the clicked file's row as selected", async () => {
    const files = [makeFileDiff({ newPath: "a.txt" }), makeFileDiff({ newPath: "b.txt" })];

    const { container, findByText } = render(CommitDiffView, { props: { files } });

    await fireEvent.click(await findByText("a.txt"));

    const selected = container.querySelector("li.selected");
    expect(selected?.textContent).toContain("a.txt");
  });
});
