// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import RemotePanel from "./RemotePanel.svelte";
import { makeBranchInfo } from "$lib/git/testFixtures";
import { settingsState } from "$lib/settings/settings.svelte";
import { toastState } from "$lib/shell/toast.svelte";
import type { RemoteProgress } from "$lib/git/types";

type ProgressChannel = { onmessage: (progress: RemoteProgress) => void };

function toastMessages(): string[] {
  return toastState.toasts.map((toast) => toast.message);
}

describe("RemotePanel", () => {
  beforeEach(() => {
    toastState.toasts = [];
    settingsState.autoFetchEnabled = false;
    settingsState.autoFetchIntervalMinutes = 5;
  });

  it("shows a warning when the installed git predates the CVE-2024-32002 fix", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.39.0", isPatched: false };
        case "list_branches":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("2.39.0");
  });

  it("fetches from origin and shows no version warning when git is patched", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "fetch": {
          const { repoPath, remoteName } = args as { repoPath: string; remoteName: string };
          calls.push({ repoPath, remoteName });
          return null;
        }
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, queryByRole } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByText("Fetch"));

    await waitFor(() => expect(toastMessages()).toContain("Fetched from origin."));
    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", remoteName: "origin" }]));
    expect(queryByRole("alert")).toBeNull();
  });

  it("streams live progress while a fetch is in flight, driven by the progress channel", async () => {
    let resolveFetch = () => {};
    const fetchGate = new Promise<void>((resolve) => {
      resolveFetch = resolve;
    });

    mockIPC(async (cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "fetch":
          (args as { progress: ProgressChannel }).progress.onmessage({
            phase: "Receiving objects",
            percent: 45,
            current: 450,
            total: 1000,
          });
          await fetchGate;
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    await fireEvent.click(await findByText("Fetch"));

    expect(await findByText("Receiving objects: 45% (450/1000)")).toBeTruthy();

    resolveFetch();
    await waitFor(() => expect(toastMessages()).toContain("Fetched from origin."));
  });

  it("shows a cancel button while busy, and cancelling surfaces a neutral status", async () => {
    const cancelCalls: unknown[] = [];
    let rejectFetch = (_reason: unknown) => {};
    const fetchGate = new Promise<void>((_resolve, reject) => {
      rejectFetch = reject;
    });

    mockIPC(async (cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "fetch":
          await fetchGate;
          return null;
        case "cancel_remote_operation":
          cancelCalls.push(args);
          rejectFetch("operation cancelled");
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, queryByText } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByText("Fetch"));
    await fireEvent.click(await findByText("Cancel"));

    await waitFor(() => expect(toastMessages()).toContain("Cancelled."));
    expect(cancelCalls).toEqual([{ repoPath: "/repo" }]);
    expect(queryByText("Cancel")).toBeNull();
  });

  it("disables pull when the current branch has no upstream configured", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true, upstream: null })];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const pullButton = (await findByText("Pull")) as HTMLButtonElement;
    await waitFor(() => expect(pullButton.disabled).toBe(true));
  });

  it("pulls and reports the outcome", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true, upstream: "origin/main" })];
        case "pull":
          return { kind: "merged" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const pullButton = (await findByText("Pull")) as HTMLButtonElement;
    await waitFor(() => expect(pullButton.disabled).toBe(false));
    await fireEvent.click(pullButton);

    await waitFor(() => expect(toastMessages()).toContain("Pulled and merged."));
  });

  it("switches to the working-directory view when a pull hits a conflict", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true, upstream: "origin/main" })];
        case "pull":
          return { kind: "conflicts", conflicts: ["a.txt"] };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onConflicts = vi.fn();
    const { findByText } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0, onConflicts },
    });

    const pullButton = (await findByText("Pull")) as HTMLButtonElement;
    await waitFor(() => expect(pullButton.disabled).toBe(false));
    await fireEvent.click(pullButton);

    await waitFor(() =>
      expect(toastMessages().some((message) => /Pull stopped with 1 conflicting/.test(message))).toBe(
        true,
      ),
    );
    await waitFor(() => expect(onConflicts).toHaveBeenCalled());
  });

  it("shows ahead/behind counts as badges on the push and pull buttons", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [
            makeBranchInfo({
              name: "main",
              isHead: true,
              upstream: "origin/main",
              ahead: 2,
              behind: 3,
            }),
          ];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByTitle } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const pullButton = await findByTitle("Pull — 3 commits behind origin");
    expect(pullButton.textContent).toContain("3");

    const pushButton = await findByTitle("Push — 2 commits ahead of origin");
    expect(pushButton.textContent).toContain("2");
  });

  it("pushes the current branch to origin", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "push": {
          const { repoPath, remoteName, branchName, force } = args as {
            repoPath: string;
            remoteName: string;
            branchName: string;
            force: boolean;
          };
          calls.push({ repoPath, remoteName, branchName, force });
          return null;
        }
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const pushButton = (await findByText("Push")) as HTMLButtonElement;
    await waitFor(() => expect(pushButton.disabled).toBe(false));
    await fireEvent.click(pushButton);

    await waitFor(() => expect(toastMessages()).toContain("Pushed main to origin."));
    await waitFor(() =>
      expect(calls).toEqual([
        { repoPath: "/repo", remoteName: "origin", branchName: "main", force: false },
      ]),
    );
  });

  it("offers a force-push retry when the push is rejected, and force-pushes on confirm", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "push": {
          const { repoPath, remoteName, branchName, force } = args as {
            repoPath: string;
            remoteName: string;
            branchName: string;
            force: boolean;
          };
          calls.push({ repoPath, remoteName, branchName, force });
          if (!force) throw "! [rejected] main -> main (non-fast-forward)";
          return null;
        }
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByText, findByRole } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const pushButton = (await findByText("Push")) as HTMLButtonElement;
    await waitFor(() => expect(pushButton.disabled).toBe(false));
    await fireEvent.click(pushButton);

    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(toastMessages()).toContain("Force-pushed main to origin."));
    expect(calls).toEqual([
      { repoPath: "/repo", remoteName: "origin", branchName: "main", force: false },
      { repoPath: "/repo", remoteName: "origin", branchName: "main", force: true },
    ]);
  });

  it("does not force-push when the user declines, and surfaces the rejection", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "push": {
          const { repoPath, remoteName, branchName, force } = args as {
            repoPath: string;
            remoteName: string;
            branchName: string;
            force: boolean;
          };
          calls.push({ repoPath, remoteName, branchName, force });
          throw "! [rejected] main -> main (non-fast-forward)";
        }
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByText, findByRole } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const pushButton = (await findByText("Push")) as HTMLButtonElement;
    await waitFor(() => expect(pushButton.disabled).toBe(false));
    await fireEvent.click(pushButton);

    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );

    await waitFor(() =>
      expect(toastMessages().some((message) => message.includes("rejected"))).toBe(true),
    );
    expect(calls).toEqual([
      { repoPath: "/repo", remoteName: "origin", branchName: "main", force: false },
    ]);
  });

  it("surfaces a non-rejection push error without offering to force", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "check_git_version":
          return { version: "2.45.1", isPatched: true };
        case "list_branches":
          return [makeBranchInfo({ name: "main", isHead: true })];
        case "push":
          throw "git error: could not resolve host";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByText, queryByRole } = render(RemotePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const pushButton = (await findByText("Push")) as HTMLButtonElement;
    await waitFor(() => expect(pushButton.disabled).toBe(false));
    await fireEvent.click(pushButton);

    await waitFor(() =>
      expect(toastMessages().some((message) => message.includes("could not resolve host"))).toBe(
        true,
      ),
    );
    expect(queryByRole("alertdialog")).toBeNull();
  });

  describe("auto-fetch", () => {
    afterEach(() => {
      vi.useRealTimers();
    });

    it("does not fetch on a timer when the setting is off (the default)", async () => {
      const fetchCalls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "check_git_version":
            return { version: "2.45.1", isPatched: true };
          case "list_branches":
            return [makeBranchInfo({ name: "main", isHead: true })];
          case "fetch":
            fetchCalls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      vi.useFakeTimers();
      render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });
      await vi.advanceTimersByTimeAsync(60 * 60_000); // well past any configurable interval

      expect(fetchCalls).toEqual([]);
    });

    it("fetches on the configured interval when enabled, silently on success", async () => {
      settingsState.autoFetchEnabled = true;
      settingsState.autoFetchIntervalMinutes = 5;
      const fetchCalls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "check_git_version":
            return { version: "2.45.1", isPatched: true };
          case "list_branches":
            return [makeBranchInfo({ name: "main", isHead: true, behind: fetchCalls.length })];
          case "fetch": {
            const { repoPath, remoteName } = args as { repoPath: string; remoteName: string };
            fetchCalls.push({ repoPath, remoteName });
            return null;
          }
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      vi.useFakeTimers();
      render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });
      await vi.advanceTimersByTimeAsync(5 * 60_000);

      expect(fetchCalls).toEqual([{ repoPath: "/repo", remoteName: "origin" }]);
      expect(toastMessages()).toEqual([]);
    });

    it("skips a tick while a manual fetch is already in flight", async () => {
      settingsState.autoFetchEnabled = true;
      settingsState.autoFetchIntervalMinutes = 5;
      let resolveManualFetch = () => {};
      const manualFetchGate = new Promise<void>((resolve) => {
        resolveManualFetch = resolve;
      });
      const fetchCalls: unknown[] = [];
      let manualFetchInFlight = false;
      mockIPC(async (cmd, args) => {
        switch (cmd) {
          case "check_git_version":
            return { version: "2.45.1", isPatched: true };
          case "list_branches":
            return [makeBranchInfo({ name: "main", isHead: true })];
          case "fetch": {
            const { repoPath, remoteName } = args as { repoPath: string; remoteName: string };
            fetchCalls.push({ repoPath, remoteName });
            if (manualFetchInFlight) await manualFetchGate;
            return null;
          }
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      vi.useFakeTimers();
      const { findByText } = render(RemotePanel, { props: { repoPath: "/repo", refreshKey: 0 } });
      manualFetchInFlight = true;
      await fireEvent.click(await findByText("Fetch"));

      // The auto-fetch tick lands while the manual fetch above is still pending.
      await vi.advanceTimersByTimeAsync(5 * 60_000);
      expect(fetchCalls).toHaveLength(1); // the manual fetch only — the tick was skipped

      resolveManualFetch();
      await waitFor(() => expect(toastMessages()).toContain("Fetched from origin."));
    });
  });
});
