// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { TreeEntry } from "$lib/git/types";
import FileTree from "./FileTree.svelte";
import { exploreState, resetExplore } from "./explore.svelte";

function entry(path: string, kind: TreeEntry["kind"], extra: Partial<TreeEntry> = {}): TreeEntry {
  return {
    name: path.split("/").pop()!,
    path,
    kind,
    size: null,
    status: null,
    containsChanges: false,
    ...extra,
  };
}

const listings: Record<string, TreeEntry[]> = {
  "": [entry("src", "dir", { containsChanges: true }), entry("README.md", "file")],
  src: [
    entry("src/lib.rs", "file", { status: "modified" }),
    entry("src/old.rs", "file", { status: "deleted" }),
  ],
};

describe("FileTree", () => {
  beforeEach(() => {
    resetExplore();
    mockIPC((cmd, args) => {
      if (cmd === "list_tree") return listings[(args as { dir: string }).dir];
    });
  });

  afterEach(() => clearMocks());

  it("lists the root, expands a folder and opens a file with its status", async () => {
    render(FileTree, { repoPath: "/repo", refreshKey: 0 });

    const src = await screen.findByRole("treeitem", { name: /src/ });
    expect(src.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByRole("img", { name: "Contains changes" })).toBeTruthy();

    await fireEvent.click(src);
    const lib = await screen.findByRole("treeitem", { name: /lib\.rs/ });
    expect(src.getAttribute("aria-expanded")).toBe("true");
    expect(lib.getAttribute("aria-level")).toBe("2");

    await fireEvent.click(lib);
    expect(exploreState.path).toBe("src/lib.rs");
    expect(exploreState.status).toBe("modified");
    expect(lib.getAttribute("aria-selected")).toBe("true");
  });

  it("moves focus with the arrow keys and opens with Enter", async () => {
    render(FileTree, { repoPath: "/repo", refreshKey: 0 });
    const tree = screen.getByRole("tree");
    await screen.findByRole("treeitem", { name: /README/ });

    await fireEvent.keyDown(tree, { key: "ArrowRight" });
    await screen.findByRole("treeitem", { name: /lib\.rs/ });
    await fireEvent.keyDown(tree, { key: "ArrowDown" });
    await fireEvent.keyDown(tree, { key: "Enter" });

    await waitFor(() => expect(exploreState.path).toBe("src/lib.rs"));
  });

  it("shows an inline error for a folder that can't be read", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "list_tree") return;
      if ((args as { dir: string }).dir === "src") throw "permission denied";
      return listings[""];
    });
    render(FileTree, { repoPath: "/repo", refreshKey: 0 });

    await fireEvent.click(await screen.findByRole("treeitem", { name: /src/ }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "Couldn't read this folder: permission denied",
    );
  });
});
