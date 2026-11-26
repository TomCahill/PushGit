// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { BlobView } from "$lib/git/types";
import FileView from "./FileView.svelte";
import { browse, exploreState, openFile, resetExplore } from "./explore.svelte";

function props() {
  return {
    repoPath: "/repo",
    refreshKey: 0,
    watchActive: true,
    onBack: vi.fn(),
    onRefresh: vi.fn(),
    onShowDiff: vi.fn(),
    onBlame: vi.fn(),
    onResolveConflict: vi.fn(),
    onOpenSubmodule: vi.fn(),
  };
}

function serve(view: BlobView, extra: Record<string, unknown> = {}) {
  mockIPC((cmd) => {
    if (cmd === "read_blob_at") return view;
    if (cmd in extra) return extra[cmd];
    if (cmd === "ref_status") return { kind: "unchanged" };
  });
}

describe("FileView", () => {
  beforeEach(() => resetExplore());
  afterEach(() => clearMocks());

  it("asks you to pick a file when none is open", () => {
    serve({ kind: "deleted" });
    render(FileView, props());
    expect(screen.getByText(/Pick a file from the tree/)).toBeTruthy();
  });

  it("shows text with line numbers and the working-tree actions", async () => {
    serve({ kind: "text", content: "one\ntwo\n", truncated: false, byteLen: 8 });
    openFile("notes.txt", "untracked");
    const p = props();
    render(FileView, p);

    expect(await screen.findByText(/one\s+two/)).toBeTruthy();
    expect(document.querySelector(".numbers")?.textContent).toBe("1\n2");
    expect(
      (screen.getByRole("button", { name: "Blame & history at HEAD" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Diff vs HEAD" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(screen.getByText("Live")).toBeTruthy();
  });

  it("offers View at HEAD for a file deleted from disk", async () => {
    serve({ kind: "deleted" }, { resolve_rev: "a".repeat(40) });
    openFile("gone.rs", "deleted");
    render(FileView, props());

    await fireEvent.click(await screen.findByRole("button", { name: "View at HEAD" }));

    await vi.waitFor(() =>
      expect(exploreState.source).toEqual({ kind: "revision", oid: "a".repeat(40) }),
    );
    expect(exploreState.refLabel).toBe("HEAD");
  });

  it("shows a moved chip and refreshes to the new commit", async () => {
    const newer = "b".repeat(40);
    serve(
      { kind: "text", content: "x\n", truncated: false, byteLen: 2 },
      { ref_status: { kind: "moved", oid: newer, newerCommits: 2 } },
    );
    browse({ kind: "revision", oid: "c".repeat(40) }, "main", "a.txt");
    render(FileView, props());

    expect(await screen.findByText(/main has moved · 2 newer commits/)).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(exploreState.source).toEqual({ kind: "revision", oid: newer });
  });

  it("shows a symlink's target, never its contents", async () => {
    serve({ kind: "symlink", target: "/home/me/.ssh/id_ed25519" });
    openFile("key", null);
    render(FileView, props());
    expect(await screen.findByText("→ /home/me/.ssh/id_ed25519")).toBeTruthy();
  });
});
