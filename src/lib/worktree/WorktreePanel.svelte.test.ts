// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import WorktreePanel from "./WorktreePanel.svelte";
import { makeWorktreeInfo } from "$lib/git/testFixtures";

const noop = () => {};

function mockNoBranches(cmd: string): unknown {
  switch (cmd) {
    case "list_branches":
    case "list_remote_branches":
      return [];
    default:
      throw new Error(`unexpected command ${cmd}`);
  }
}

describe("WorktreePanel", () => {
  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { queryByText } = render(WorktreePanel, {
      props: { repoPath: "", refreshKey: 0, onOpen: noop },
    });

    expect(queryByText("(main)")).toBeNull();
  });

  it("loads and renders the main worktree and a linked one", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "feature-wt",
            path: "/repo-worktrees/feature",
            branch: "feature",
            isMain: false,
          }),
        ];
      }
      return mockNoBranches(cmd);
    });

    const { findByText } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    expect(await findByText("main")).toBeTruthy();
    expect(await findByText("feature")).toBeTruthy();
  });

  it("shows the main entry without a Remove button", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") return [makeWorktreeInfo()];
      return mockNoBranches(cmd);
    });

    const { findByText, queryByRole } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("main");
    expect(queryByRole("button", { name: "Remove" })).toBeNull();
  });

  it("disables Open and Remove for the entry currently open, but not for others", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "feature-wt",
            path: "/repo-worktrees/feature",
            branch: "feature",
            isMain: false,
          }),
        ];
      }
      return mockNoBranches(cmd);
    });

    // The currently-open path is the *linked* worktree here, not main — so this exercises a
    // non-main "Current" row, which is the only case with both a Remove button to disable and
    // an Open button to disable.
    const { findAllByRole, findByText } = render(WorktreePanel, {
      props: { repoPath: "/repo-worktrees/feature", refreshKey: 0, onOpen: noop },
    });

    await findByText("Current");
    const openButtons = (await findAllByRole("button", { name: "Open" })) as HTMLButtonElement[];
    expect(openButtons[0].disabled).toBe(false); // main, not current
    expect(openButtons[1].disabled).toBe(true); // feature, current

    const removeButtons = (await findAllByRole("button", {
      name: "Remove",
    })) as HTMLButtonElement[];
    expect(removeButtons).toHaveLength(1); // main never has one, current or not
    expect(removeButtons[0].disabled).toBe(true);
  });

  it("disables Open and shows a missing badge for a worktree whose directory is gone", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "gone-wt",
            path: "/repo-worktrees/gone",
            branch: null,
            isMain: false,
            isMissing: true,
          }),
        ];
      }
      return mockNoBranches(cmd);
    });

    const { findByText, findAllByRole } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("Missing");
    const openButtons = (await findAllByRole("button", { name: "Open" })) as HTMLButtonElement[];
    expect(openButtons[1].disabled).toBe(true);
  });

  it("calls onOpen with the target path when Open is clicked", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "feature-wt",
            path: "/repo-worktrees/feature",
            branch: "feature",
            isMain: false,
          }),
        ];
      }
      return mockNoBranches(cmd);
    });

    const onOpen = vi.fn();
    const { findAllByRole, findByText } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen },
    });

    await findByText("feature");
    const openButtons = await findAllByRole("button", { name: "Open" });
    await fireEvent.click(openButtons[1]);

    expect(onOpen).toHaveBeenCalledWith("/repo-worktrees/feature");
  });

  it("shows a stronger confirmation for removing a dirty worktree", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "feature-wt",
            path: "/repo-worktrees/feature",
            branch: "feature",
            isMain: false,
            isDirty: true,
          }),
        ];
      }
      return mockNoBranches(cmd);
    });

    render(ConfirmDialog);
    const { findByText, findByRole } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("Uncommitted changes");
    await fireEvent.click(await findByText("Remove"));

    const dialog = await findByRole("alertdialog");
    expect(dialog.textContent).toContain("has uncommitted changes that will be lost");
  });

  it("removes a worktree only after the user confirms", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_worktrees") {
        return [
          makeWorktreeInfo({ name: "(main)", path: "/repo", branch: "main", isMain: true }),
          makeWorktreeInfo({
            name: "feature-wt",
            path: "/repo-worktrees/feature",
            branch: "feature",
            isMain: false,
          }),
        ];
      }
      if (cmd === "remove_worktree") {
        calls.push(args);
        return null;
      }
      return mockNoBranches(cmd);
    });

    render(ConfirmDialog);
    const { findByText, findByRole } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("feature");
    await fireEvent.click(await findByText("Remove"));
    let dialogEl = await findByRole("alertdialog");
    expect(dialogEl.textContent).toContain("Remove worktree");
    await fireEvent.click(within(dialogEl).getByRole("button", { name: "Cancel" }));
    expect(calls).toEqual([]);

    await fireEvent.click(await findByText("Remove"));
    dialogEl = await findByRole("alertdialog");
    await fireEvent.click(within(dialogEl).getByRole("button", { name: "OK" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "feature-wt" }]));
  });

  it("defaults Location from Branch until the user edits Location directly", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") return [makeWorktreeInfo()];
      return mockNoBranches(cmd);
    });

    const { findByLabelText } = render(WorktreePanel, {
      props: { repoPath: "/home/user/myrepo", refreshKey: 0, onOpen: noop },
    });

    const branchInput = (await findByLabelText("Branch")) as HTMLInputElement;
    const locationInput = (await findByLabelText("Location")) as HTMLInputElement;

    await fireEvent.input(branchInput, { target: { value: "feature-x" } });
    await waitFor(() =>
      expect(locationInput.value).toBe("/home/user/myrepo-worktrees/feature-x"),
    );

    await fireEvent.input(locationInput, { target: { value: "/custom/path" } });
    expect(locationInput.value).toBe("/custom/path");

    await fireEvent.input(branchInput, { target: { value: "another-branch" } });
    expect(locationInput.value).toBe("/custom/path");
  });

  it("creates a worktree via the form and opens it", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_worktrees") return [makeWorktreeInfo()];
      if (cmd === "add_worktree") {
        calls.push(args);
        return null;
      }
      return mockNoBranches(cmd);
    });

    const onOpen = vi.fn();
    const { findByLabelText, findByRole } = render(WorktreePanel, {
      props: { repoPath: "/home/user/myrepo", refreshKey: 0, onOpen },
    });

    const branchInput = (await findByLabelText("Branch")) as HTMLInputElement;
    await fireEvent.input(branchInput, { target: { value: "feature-x" } });
    await fireEvent.click(await findByRole("button", { name: "Create Worktree" }));

    await waitFor(() =>
      expect(calls).toEqual([
        {
          repoPath: "/home/user/myrepo",
          branchName: "feature-x",
          startPoint: undefined,
          path: "/home/user/myrepo-worktrees/feature-x",
        },
      ]),
    );
    await waitFor(() =>
      expect(onOpen).toHaveBeenCalledWith("/home/user/myrepo-worktrees/feature-x"),
    );
    await waitFor(() => expect(branchInput.value).toBe(""));
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_worktrees") throw "git error: not a repository";
      return mockNoBranches(cmd);
    });

    const { findByRole } = render(WorktreePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
