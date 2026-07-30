// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import InteractiveRebaseEditor from "./InteractiveRebaseEditor.svelte";
import { makeRebaseCommitSummary } from "$lib/git/testFixtures";

describe("InteractiveRebaseEditor", () => {
  it("renders every commit as pick by default, oldest first as given", async () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const commits = [
      makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" }),
      makeRebaseCommitSummary({ oid: "bbb", shortOid: "bbb", summary: "second" }),
    ];
    const { findByText } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted: vi.fn(),
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    expect(await findByText("first")).toBeTruthy();
    expect(await findByText("second")).toBeTruthy();
  });

  it("starts a rebase with the default pick steps and reports completion", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          calls.push(args);
          return { kind: "completed" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onCompleted = vi.fn();
    const commits = [makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" })];
    const { findByRole } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted,
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));

    await waitFor(() =>
      expect(calls).toEqual([
        {
          repoPath: "/repo",
          onto: "main",
          steps: [{ oid: "aaa", action: { kind: "pick" } }],
        },
      ]),
    );
    await waitFor(() => expect(onCompleted).toHaveBeenCalled());
  });

  it("reorders commits via drag and drop before starting", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          calls.push(args);
          return { kind: "completed" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const commits = [
      makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" }),
      makeRebaseCommitSummary({ oid: "bbb", shortOid: "bbb", summary: "second" }),
    ];
    const { findByText, findByRole } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted: vi.fn(),
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    const firstRow = (await findByText("first")).closest("li")!;
    const secondRow = (await findByText("second")).closest("li")!;

    await fireEvent.dragStart(firstRow);
    await fireEvent.dragOver(secondRow);
    await fireEvent.drop(secondRow);

    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));

    await waitFor(() =>
      expect(calls).toEqual([
        {
          repoPath: "/repo",
          onto: "main",
          steps: [
            { oid: "bbb", action: { kind: "pick" } },
            { oid: "aaa", action: { kind: "pick" } },
          ],
        },
      ]),
    );
  });

  it("sends a drop step when Drop is selected", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          calls.push(args);
          return { kind: "completed" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const commits = [makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" })];
    const { findByRole } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted: vi.fn(),
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    await fireEvent.click(await findByRole("button", { name: "Drop" }));
    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));

    await waitFor(() =>
      expect(calls).toEqual([
        { repoPath: "/repo", onto: "main", steps: [{ oid: "aaa", action: { kind: "drop" } }] },
      ]),
    );
  });

  it("sends a reword step with the edited message", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          calls.push(args);
          return { kind: "completed" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const commits = [makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" })];
    const { findByRole, findByLabelText } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted: vi.fn(),
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    await fireEvent.click(await findByRole("button", { name: "Reword" }));
    const textarea = await findByLabelText("New message for aaa");
    await fireEvent.input(textarea, { target: { value: "a better message" } });
    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));

    await waitFor(() =>
      expect(calls).toEqual([
        {
          repoPath: "/repo",
          onto: "main",
          steps: [{ oid: "aaa", action: { kind: "reword", message: "a better message" } }],
        },
      ]),
    );
  });

  it("shows a conflict banner and can continue or abort via the interactive-specific commands", async () => {
    const continueCalls: unknown[] = [];
    const abortCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          return { kind: "conflicts", conflicts: ["a.txt"] };
        case "continue_interactive_rebase":
          continueCalls.push(args);
          return { kind: "completed" };
        case "abort_interactive_rebase":
          abortCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onConflicts = vi.fn();
    const onCompleted = vi.fn();
    const commits = [makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" })];
    const { findByRole, findByText } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted,
        onConflicts,
        onCancel: vi.fn(),
      },
    });

    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));

    expect(await findByText(/1 conflicting file/)).toBeTruthy();
    await waitFor(() => expect(onConflicts).toHaveBeenCalled());

    await fireEvent.click(await findByRole("button", { name: "Continue rebase" }));
    await waitFor(() => expect(continueCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(onCompleted).toHaveBeenCalled());
  });

  it("aborts a paused rebase", async () => {
    const abortCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "start_interactive_rebase":
          return { kind: "conflicts", conflicts: ["a.txt"] };
        case "abort_interactive_rebase":
          abortCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onCancel = vi.fn();
    const commits = [makeRebaseCommitSummary({ oid: "aaa", shortOid: "aaa", summary: "first" })];
    const { findByRole } = render(InteractiveRebaseEditor, {
      props: {
        repoPath: "/repo",
        onto: "main",
        commits,
        onCompleted: vi.fn(),
        onConflicts: vi.fn(),
        onCancel,
      },
    });

    await fireEvent.click(await findByRole("button", { name: "Start Rebase" }));
    await findByRole("button", { name: "Abort rebase" });
    await fireEvent.click(await findByRole("button", { name: "Abort rebase" }));

    await waitFor(() => expect(abortCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
  });
});
