// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import BlameView from "./BlameView.svelte";
import { makeFileDiff } from "$lib/git/testFixtures";

describe("BlameView", () => {
  it("renders file history and reports blame lines up via onLinesChange, not inline", async () => {
    const line = {
      lineNo: 1,
      content: "line one",
      oid: "aaa",
      shortOid: "aaa",
      authorName: "Ada",
      authorTime: 1700000000,
      summary: "first",
    };
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "blame_file":
          expect(args).toEqual({ repoPath: "/repo", path: "a.txt" });
          return [line];
        case "file_history":
          return [
            {
              oid: "aaa",
              shortOid: "aaa",
              summary: "first",
              authorName: "Ada",
              authorTime: 1700000000,
            },
          ];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onLinesChange = vi.fn();
    const { findByText, queryByText } = render(BlameView, {
      props: { repoPath: "/repo", path: "a.txt", onClose: vi.fn(), onLinesChange },
    });

    expect(await findByText("History (1)")).toBeTruthy();
    expect(await findByText("first")).toBeTruthy();
    await waitFor(() => expect(onLinesChange).toHaveBeenCalledWith([line]));
    // The center panel (owned by the shell), not this view, renders the annotated file.
    expect(queryByText("line one")).toBeNull();
  });

  it("reports the selected history entry's diff up via onDiffChange, not inline", async () => {
    const fileDiff = makeFileDiff({ newPath: "a.txt" });
    mockIPC((cmd) => {
      switch (cmd) {
        case "blame_file":
          return [];
        case "file_history":
          return [
            {
              oid: "aaa",
              shortOid: "aaa",
              summary: "first",
              authorName: "Ada",
              authorTime: 1700000000,
            },
          ];
        case "diff_commit":
          return [fileDiff];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onDiffChange = vi.fn();
    const { findByText, queryByText } = render(BlameView, {
      props: { repoPath: "/repo", path: "a.txt", onClose: vi.fn(), onDiffChange },
    });

    await fireEvent.click(await findByText("first"));

    await waitFor(() => expect(onDiffChange).toHaveBeenCalledWith({ file: fileDiff }));
    // The center panel (owned by the shell), not this view, renders the diff itself.
    expect(queryByText("hello", { exact: false })).toBeNull();
  });

  it("calls onClose when the Close button is clicked", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "blame_file":
          return [];
        case "file_history":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onClose = vi.fn();
    const { findByRole } = render(BlameView, {
      props: { repoPath: "/repo", path: "a.txt", onClose },
    });

    await fireEvent.click(await findByRole("button", { name: "Close" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("surfaces a load error", async () => {
    mockIPC((cmd) => {
      if (cmd === "blame_file") throw new Error("no such path");
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(BlameView, {
      props: { repoPath: "/repo", path: "missing.txt", onClose: vi.fn() },
    });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("no such path");
  });
});
