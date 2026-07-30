// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConflictEditor from "./ConflictEditor.svelte";
import { makeConflictSides } from "$lib/git/testFixtures";

describe("ConflictEditor", () => {
  it("loads and renders the hunk with per-hunk resolution controls", async () => {
    mockIPC((cmd) => {
      if (cmd === "conflict_sides") return makeConflictSides();
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved: vi.fn(), onCancel: vi.fn() },
    });

    expect(await findByText("@@ -1,1 +1,1 @@")).toBeTruthy();
    expect(await findByText("Take ours")).toBeTruthy();
    expect(await findByText("Take theirs")).toBeTruthy();
    expect(await findByText("Edit manually")).toBeTruthy();
  });

  it("disables Mark resolved until every hunk has a decision", async () => {
    mockIPC((cmd) => {
      if (cmd === "conflict_sides") return makeConflictSides();
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved: vi.fn(), onCancel: vi.fn() },
    });
    await findByText("Take ours");

    expect((getByText("Mark resolved") as HTMLButtonElement).disabled).toBe(true);

    await fireEvent.click(getByText("Take ours"));

    expect((getByText("Mark resolved") as HTMLButtonElement).disabled).toBe(false);
  });

  it("marks resolved with ours' content when 'Take ours' is chosen", async () => {
    const calls: unknown[] = [];
    const onResolved = vi.fn();
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "conflict_sides":
          return makeConflictSides();
        case "write_resolved_conflict":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved, onCancel: vi.fn() },
    });
    await findByText("Take ours");

    await fireEvent.click(getByText("Take ours"));
    await fireEvent.click(getByText("Mark resolved"));

    await waitFor(() => expect(onResolved).toHaveBeenCalled());
    expect(calls).toEqual([{ repoPath: "/repo", path: "shared.txt", content: "main version\n" }]);
  });

  it("marks resolved with theirs' content when 'Take theirs' is chosen", async () => {
    const calls: unknown[] = [];
    const onResolved = vi.fn();
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "conflict_sides":
          return makeConflictSides();
        case "write_resolved_conflict":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved, onCancel: vi.fn() },
    });
    await findByText("Take theirs");

    await fireEvent.click(getByText("Take theirs"));
    await fireEvent.click(getByText("Mark resolved"));

    await waitFor(() => expect(onResolved).toHaveBeenCalled());
    expect(calls).toEqual([
      { repoPath: "/repo", path: "shared.txt", content: "feature version\n" },
    ]);
  });

  it("resolves with manually-edited content when 'Edit manually' is used", async () => {
    const calls: unknown[] = [];
    const onResolved = vi.fn();
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "conflict_sides":
          return makeConflictSides();
        case "write_resolved_conflict":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText, getByRole } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved, onCancel: vi.fn() },
    });
    await findByText("Edit manually");

    await fireEvent.click(getByText("Edit manually"));
    const textarea = getByRole("textbox") as HTMLTextAreaElement;
    expect(textarea.value).toBe("feature version\n");
    await fireEvent.input(textarea, { target: { value: "merged by hand\n" } });

    await fireEvent.click(getByText("Mark resolved"));

    await waitFor(() => expect(onResolved).toHaveBeenCalled());
    expect(calls).toEqual([{ repoPath: "/repo", path: "shared.txt", content: "merged by hand\n" }]);
  });

  it("calls onCancel when Cancel is clicked", async () => {
    const onCancel = vi.fn();
    mockIPC((cmd) => {
      if (cmd === "conflict_sides") return makeConflictSides();
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved: vi.fn(), onCancel },
    });
    await findByText("Cancel");

    await fireEvent.click(getByText("Cancel"));

    expect(onCancel).toHaveBeenCalled();
  });

  it("offers whole-file keep-ours/keep-theirs for a binary conflict", async () => {
    const calls: unknown[] = [];
    const onResolved = vi.fn();
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "conflict_sides":
          return makeConflictSides({ isBinary: true, hunks: [] });
        case "write_resolved_conflict":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "image.png", onResolved, onCancel: vi.fn() },
    });
    await findByText("Keep ours");

    await fireEvent.click(getByText("Keep theirs"));

    await waitFor(() => expect(onResolved).toHaveBeenCalled());
    expect(calls).toEqual([{ repoPath: "/repo", path: "image.png", content: "feature version\n" }]);
  });

  it("offers keep/delete for a delete/modify conflict and can delete the path", async () => {
    const calls: unknown[] = [];
    const onResolved = vi.fn();
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "conflict_sides":
          return makeConflictSides({ ours: null, hunks: [] });
        case "resolve_conflict_as_deleted":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText } = render(ConflictEditor, {
      props: { repoPath: "/repo", path: "shared.txt", onResolved, onCancel: vi.fn() },
    });
    await findByText("Delete the file");

    await fireEvent.click(getByText("Delete the file"));

    await waitFor(() => expect(onResolved).toHaveBeenCalled());
    expect(calls).toEqual([{ repoPath: "/repo", path: "shared.txt" }]);
  });
});
