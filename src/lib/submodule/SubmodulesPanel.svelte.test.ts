// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import SubmodulesPanel from "./SubmodulesPanel.svelte";
import { makeSubmoduleInfo } from "$lib/git/testFixtures";
import { toastState } from "$lib/shell/toast.svelte";
import type { RemoteProgress } from "$lib/git/types";

type ProgressChannel = { onmessage: (progress: RemoteProgress) => void };

const noop = () => {};

function toastMessages(): string[] {
  return toastState.toasts.map((toast) => toast.message);
}

describe("SubmodulesPanel", () => {
  beforeEach(() => {
    toastState.toasts = [];
  });

  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { queryByText } = render(SubmodulesPanel, {
      props: { repoPath: "", refreshKey: 0, onOpen: noop },
    });

    expect(queryByText("vendor/lib")).toBeNull();
  });

  it("shows an empty-state message when the repo has no submodules", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") return [];
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    expect(await findByText("No submodules in this repository.")).toBeTruthy();
  });

  it("renders a submodule whose name matches its path without repeating the path", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findAllByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo/", refreshKey: 0, onOpen: noop },
    });

    expect(await findAllByText("vendor/lib")).toHaveLength(1);
  });

  it("shows the path separately when it differs from the name", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "lib", path: "vendor/lib" })];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo/", refreshKey: 0, onOpen: noop },
    });

    expect(await findByText("lib")).toBeTruthy();
    expect(await findByText("vendor/lib")).toBeTruthy();
  });

  it("shows Init only for an uninitialized submodule", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [
          makeSubmoduleInfo({ name: "vendor/a", isInitialized: false }),
          makeSubmoduleInfo({ name: "vendor/b", isInitialized: true }),
        ];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText, findAllByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("vendor/a");
    const initButtons = await findAllByRole("button", { name: "Init" });
    expect(initButtons).toHaveLength(1);
  });

  it("disables Open and shows a Missing badge for an initialized submodule with nothing checked out", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "vendor/lib", isMissing: true, isInitialized: true })];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText, findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("Missing");
    const openButton = (await findByRole("button", { name: "Open" })) as HTMLButtonElement;
    expect(openButton.disabled).toBe(true);
  });

  it("shows only Not initialized for a never-cloned submodule, not Missing as well", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "vendor/lib", isMissing: true, isInitialized: false })];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText, findByRole, queryByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await findByText("Not initialized");
    expect(queryByText("Missing")).toBeNull();
    const openButton = (await findByRole("button", { name: "Open" })) as HTMLButtonElement;
    expect(openButton.disabled).toBe(true);
  });

  it("shows Needs update and Uncommitted changes badges simultaneously", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "vendor/lib", needsUpdate: true, isDirty: true })];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    expect(await findByText("Needs update")).toBeTruthy();
    expect(await findByText("Uncommitted changes")).toBeTruthy();
  });

  it("opens the submodule at its absolute path, not the repo-relative one git reports", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "vendor/lib", path: "vendor/lib" })];
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const onOpen = vi.fn();
    const { findByText, findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/home/user/repo/", refreshKey: 0, onOpen },
    });

    await findByText("vendor/lib");
    await fireEvent.click(await findByRole("button", { name: "Open" }));

    expect(onOpen).toHaveBeenCalledWith("/home/user/repo/vendor/lib");
  });

  it("initializes a submodule and refreshes the list", async () => {
    const calls: unknown[] = [];
    let initialized = false;
    mockIPC((cmd, args) => {
      if (cmd === "list_submodules") {
        return [makeSubmoduleInfo({ name: "vendor/lib", isInitialized: initialized })];
      }
      if (cmd === "init_submodule") {
        calls.push(args);
        initialized = true;
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole, queryByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await fireEvent.click(await findByRole("button", { name: "Init" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "vendor/lib" }]));
    await waitFor(() => expect(toastMessages()).toContain('Initialized "vendor/lib".'));
    await waitFor(() => expect(queryByRole("button", { name: "Init" })).toBeNull());
  });

  it("syncs a submodule's URL to .gitmodules", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      if (cmd === "sync_submodule") {
        calls.push(args);
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await fireEvent.click(await findByRole("button", { name: "Sync" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "vendor/lib" }]));
    await waitFor(() => expect(toastMessages()).toContain('Synced "vendor/lib" to .gitmodules.'));
  });

  it("streams live progress while a per-row update is in flight", async () => {
    let resolveUpdate = () => {};
    const updateGate = new Promise<void>((resolve) => {
      resolveUpdate = resolve;
    });
    const calls: unknown[] = [];

    mockIPC(async (cmd, args) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      if (cmd === "update_submodule") {
        calls.push(args);
        (args as { progress: ProgressChannel }).progress.onmessage({
          phase: "Receiving objects",
          percent: 60,
          current: 60,
          total: 100,
        });
        await updateGate;
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole, findByText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await fireEvent.click(await findByRole("button", { name: "Update" }));

    expect(await findByText("Receiving objects: 60% (60/100)")).toBeTruthy();
    expect(calls).toEqual([
      { repoPath: "/repo", name: "vendor/lib", recursive: false, progress: expect.anything() },
    ]);

    resolveUpdate();
    await waitFor(() => expect(toastMessages()).toContain('Updated "vendor/lib".'));
  });

  it("updates every submodule via 'Update all' with name undefined", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      if (cmd === "update_submodule") {
        calls.push(args);
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await fireEvent.click(await findByRole("button", { name: "Update all" }));

    await waitFor(() =>
      expect(calls).toEqual([
        { repoPath: "/repo", name: undefined, recursive: false, progress: expect.anything() },
      ]),
    );
  });

  it("cancels through its own command, not the fetch/pull/push one", async () => {
    let rejectUpdate: (reason: unknown) => void = () => {};
    const cancelCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      if (cmd === "update_submodule") {
        return new Promise((_resolve, reject) => {
          rejectUpdate = reject;
        });
      }
      if (cmd === "cancel_submodule_update") {
        cancelCalls.push(args);
        rejectUpdate("operation cancelled");
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    await fireEvent.click(await findByRole("button", { name: "Update all" }));
    await fireEvent.click(await findByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(cancelCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(toastMessages()).toContain("Cancelled."));
  });

  it("passes recursive: true when the checkbox is checked", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "list_submodules") return [makeSubmoduleInfo({ name: "vendor/lib" })];
      if (cmd === "update_submodule") {
        calls.push(args);
        return null;
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole, findByLabelText } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    const checkbox = await findByLabelText("Recursive (include nested submodules)");
    await fireEvent.click(checkbox);
    await fireEvent.click(await findByRole("button", { name: "Update all" }));

    await waitFor(() =>
      expect(calls).toEqual([
        { repoPath: "/repo", name: undefined, recursive: true, progress: expect.anything() },
      ]),
    );
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_submodules") throw "git error: not a repository";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(SubmodulesPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onOpen: noop },
    });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
