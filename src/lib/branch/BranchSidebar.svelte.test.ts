// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import BranchSidebar from "./BranchSidebar.svelte";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import { makeBranchInfo } from "$lib/git/testFixtures";
import { toastState } from "$lib/shell/toast.svelte";

describe("BranchSidebar", () => {
  beforeEach(() => {
    toastState.toasts = [];
  });


  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { getByText } = render(BranchSidebar, { props: { repoPath: "", refreshKey: 0 } });

    expect(getByText("No branches.")).toBeTruthy();
  });

  it("loads and renders the branch list with a HEAD marker and ahead/behind counts", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature", upstream: "origin/feature", ahead: 2, behind: 1 }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText("main")).toBeTruthy();
    expect(await findByText("feature")).toBeTruthy();
    expect(await findByText("HEAD")).toBeTruthy();
    expect(await findByText("↑2 ↓1")).toBeTruthy();
  });

  it("copies a branch name without checking it out", async () => {
    const checkoutCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "checkout_branch":
          checkoutCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const { findByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByRole("button", { name: "Copy branch name feature" }));

    expect(writeText).toHaveBeenCalledWith("feature");
    expect(checkoutCalls).toEqual([]);
  });

  it("checks out a branch when its row is clicked", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "checkout_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByTitle } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    await fireEvent.click(await findByTitle("Checkout feature"));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "feature" }]));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("creates a branch via the form", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "create_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const input = (await findByLabelText("New branch")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "release" } });
    await fireEvent.click(getByRole("button", { name: "Create" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "release", at: null }]));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("deletes a branch only after the user confirms", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "delete_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle("Delete feature"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(calls).toEqual([]);

    await fireEvent.click(await findByTitle("Delete feature"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "feature" }]));
  });

  it("renames a branch via a prompt, including the current (HEAD) branch", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "rename_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle("Rename main"));
    const dialog = within(await findByRole("alertdialog"));
    const input = dialog.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("main");
    await fireEvent.input(input, { target: { value: "main-renamed" } });
    await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", oldName: "main", newName: "main-renamed" }]),
    );
  });

  it("does nothing when the rename prompt is cancelled or unchanged", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "rename_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    // Cancelled.
    await fireEvent.click(await findByTitle("Rename main"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );

    // Submitted unchanged — the dialog pre-fills the current name, and the user submits
    // as-is without editing it.
    await fireEvent.click(await findByTitle("Rename main"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    expect(calls).toEqual([]);
  });

  it("merges a branch and reports the outcome", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "repository_state":
          return "clean";
        case "list_conflicts":
          return [];
        case "merge_branch":
          return { kind: "merged" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByTitle } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle("Merge feature into the current branch"));

    await waitFor(() =>
      expect(toastState.toasts.map((t) => t.message)).toContain("Merged feature."),
    );
  });

  it("shows a merge-conflict banner and can abort", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "merge";
        case "list_conflicts":
          return ["shared.txt"];
        case "abort_merge":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText(/Merge in progress — 1 conflicting file/)).toBeTruthy();
    await fireEvent.click(await findByText("Abort merge"));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo" }]));
  });

  it("shows a cherry-pick-conflict banner and can abort", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "cherry_pick";
        case "list_conflicts":
          return ["shared.txt"];
        case "is_multi_cherry_pick_in_progress":
          return false;
        case "abort_cherry_pick":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText(/Cherry-pick paused — 1 conflicting file/)).toBeTruthy();
    await fireEvent.click(await findByText("Abort cherry-pick"));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo" }]));
  });

  it("shows continue/abort (not the single-commit abort) for a paused cherry-pick range", async () => {
    const continueCalls: unknown[] = [];
    const abortCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "cherry_pick";
        case "list_conflicts":
          return ["shared.txt"];
        case "is_multi_cherry_pick_in_progress":
          return true;
        case "continue_cherry_pick_range":
          continueCalls.push(args);
          return { kind: "cherry_picked", oid: "new-oid" };
        case "abort_cherry_pick_range":
          abortCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText(/Cherry-pick range paused — 1 conflicting file/)).toBeTruthy();

    await fireEvent.click(await findByText("Continue cherry-pick"));
    await waitFor(() => expect(continueCalls).toEqual([{ repoPath: "/repo" }]));

    const abortButton = (await findByText("Abort cherry-pick")) as HTMLButtonElement;
    await waitFor(() => expect(abortButton.disabled).toBe(false));
    await fireEvent.click(abortButton);
    await waitFor(() => expect(abortCalls).toEqual([{ repoPath: "/repo" }]));
  });

  it("shows a rebase-paused banner with continue and abort actions", async () => {
    const continueCalls: unknown[] = [];
    const abortCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "rebase";
        case "list_conflicts":
          return ["shared.txt"];
        case "is_interactive_rebase_in_progress":
          return false;
        case "continue_rebase":
          continueCalls.push(args);
          return { kind: "completed" };
        case "abort_rebase":
          abortCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText(/Rebase paused — 1 conflicting file/)).toBeTruthy();

    await fireEvent.click(await findByText("Continue rebase"));
    await waitFor(() => expect(continueCalls).toEqual([{ repoPath: "/repo" }]));

    const abortButton = (await findByText("Abort rebase")) as HTMLButtonElement;
    await waitFor(() => expect(abortButton.disabled).toBe(false));
    await fireEvent.click(abortButton);
    await waitFor(() => expect(abortCalls).toEqual([{ repoPath: "/repo" }]));
  });

  it("suppresses its own continue/abort for a rebase the interactive editor started", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "repository_state":
          return "rebase";
        case "list_conflicts":
          return ["shared.txt"];
        case "is_interactive_rebase_in_progress":
          return true;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, queryByText } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText(/interactive rebase is paused/i)).toBeTruthy();
    expect(queryByText("Continue rebase")).toBeNull();
    expect(queryByText("Abort rebase")).toBeNull();
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_branches") throw "git error: not a repository";
      if (cmd === "repository_state") return "clean";
      if (cmd === "list_conflicts") return [];
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(BranchSidebar, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
