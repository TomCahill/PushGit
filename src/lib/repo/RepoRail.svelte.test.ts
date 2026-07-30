// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import ContextMenu from "$lib/shell/ContextMenu.svelte";
import { firePointer } from "$lib/shell/testPointerEvents";
import { notificationHistory, notifyError, notifySuccess, toastState } from "$lib/shell/toast.svelte";
import RepoRail from "./RepoRail.svelte";
import type { RecentRepo } from "./recentRepos";
import type { RepoFolder } from "./repoFolders";

function baseProps(overrides: Partial<Parameters<typeof RepoRail>[1]> = {}) {
  return {
    currentPath: "",
    recentRepos: [] as RecentRepo[],
    folders: [] as RepoFolder[],
    openError: null,
    onOpenPath: vi.fn(),
    onSelectRecent: vi.fn(),
    onRemoveRecent: vi.fn(),
    onCreateFolder: vi.fn(),
    onRenameFolder: vi.fn(),
    onDeleteFolder: vi.fn(),
    onToggleFolderCollapsed: vi.fn(),
    onMoveRepo: vi.fn(),
    onMoveFolder: vi.fn(),
    onOpenSettings: vi.fn(),
    ...overrides,
  };
}

describe("RepoRail", () => {
  it("shows a placeholder when there's no recent-repo history yet", () => {
    const { getByText } = render(RepoRail, { props: baseProps() });

    expect(getByText("Repos you open will show up here.")).toBeTruthy();
  });

  it("lists recent repos by their last path segment, with the full path as a tooltip", () => {
    const recentRepos: RecentRepo[] = [
      { path: "/home/tom/Projects/pushgit", lastOpenedAt: 2 },
      { path: "/home/tom/Projects/other-repo", lastOpenedAt: 1 },
    ];

    const { getByText } = render(RepoRail, { props: baseProps({ recentRepos }) });

    const pushgitRow = getByText("pushgit").closest("button");
    expect(pushgitRow?.title).toBe("/home/tom/Projects/pushgit");
    expect(getByText("other-repo")).toBeTruthy();
  });

  it("calls onSelectRecent with the full path when a recent entry is clicked", async () => {
    const onSelectRecent = vi.fn();
    const { getByText } = render(RepoRail, {
      props: baseProps({
        recentRepos: [{ path: "/home/tom/Projects/pushgit", lastOpenedAt: 1 }],
        onSelectRecent,
      }),
    });

    await fireEvent.click(getByText("pushgit"));

    expect(onSelectRecent).toHaveBeenCalledWith("/home/tom/Projects/pushgit");
  });

  it("calls onRemoveRecent (and not onSelectRecent) when the remove button is clicked", async () => {
    const onSelectRecent = vi.fn();
    const onRemoveRecent = vi.fn();
    const { getByTitle } = render(RepoRail, {
      props: baseProps({
        recentRepos: [{ path: "/home/tom/Projects/pushgit", lastOpenedAt: 1 }],
        onSelectRecent,
        onRemoveRecent,
      }),
    });

    await fireEvent.click(getByTitle("Remove /home/tom/Projects/pushgit from recent"));

    expect(onRemoveRecent).toHaveBeenCalledWith("/home/tom/Projects/pushgit");
    expect(onSelectRecent).not.toHaveBeenCalled();
  });

  it("calls onOpenPath with the picked folder when the dialog resolves", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return "/some/repo";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });
    const onOpenPath = vi.fn();
    const { getByTitle } = render(RepoRail, { props: baseProps({ onOpenPath }) });

    await fireEvent.click(getByTitle("Open a repository"));

    await waitFor(() => expect(onOpenPath).toHaveBeenCalledWith("/some/repo"));
  });

  it("does not call onOpenPath when the folder dialog is cancelled", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });
    const onOpenPath = vi.fn();
    const { getByTitle } = render(RepoRail, { props: baseProps({ onOpenPath }) });

    await fireEvent.click(getByTitle("Open a repository"));

    await Promise.resolve();
    expect(onOpenPath).not.toHaveBeenCalled();
  });

  it("surfaces an open error", () => {
    const { getByRole } = render(RepoRail, {
      props: baseProps({ openError: "git error: not a git repository" }),
    });

    expect(getByRole("alert").textContent).toContain("not a git repository");
  });

  it("marks the currently-open repo in the recent list", () => {
    const { getByText } = render(RepoRail, {
      props: baseProps({
        currentPath: "/home/tom/Projects/pushgit",
        recentRepos: [
          { path: "/home/tom/Projects/pushgit", lastOpenedAt: 2 },
          { path: "/home/tom/Projects/other-repo", lastOpenedAt: 1 },
        ],
      }),
    });

    const currentRow = getByText("pushgit").closest("li");
    const otherRow = getByText("other-repo").closest("li");
    expect(currentRow?.className).toContain("current");
    expect(otherRow?.className).not.toContain("current");
  });

  describe("folders", () => {
    const folders: RepoFolder[] = [{ id: "f1", name: "Work" }];
    const recentRepos: RecentRepo[] = [
      { path: "/repo/foldered", lastOpenedAt: 2, folderId: "f1" },
      { path: "/repo/ungrouped", lastOpenedAt: 1 },
    ];

    it("groups repos under their folder, separate from the ungrouped Recent list", () => {
      const { getByText } = render(RepoRail, { props: baseProps({ folders, recentRepos }) });

      expect(getByText("Work")).toBeTruthy();
      expect(getByText("foldered")).toBeTruthy();
      expect(getByText("ungrouped")).toBeTruthy();
      expect(getByText("1")).toBeTruthy(); // folder's repo count
    });

    it("toggles a folder collapsed when its header is clicked", async () => {
      const onToggleFolderCollapsed = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onToggleFolderCollapsed }),
      });

      await fireEvent.click(getByText("Work"));

      expect(onToggleFolderCollapsed).toHaveBeenCalledWith("f1");
    });

    it("hides a folder's repos when collapsed", () => {
      const { getByText, queryByText } = render(RepoRail, {
        props: baseProps({ folders: [{ id: "f1", name: "Work", collapsed: true }], recentRepos }),
      });

      expect(getByText("Work")).toBeTruthy();
      expect(queryByText("foldered")).toBeNull();
    });

    it("creates a folder from the New Folder button", async () => {
      const onCreateFolder = vi.fn().mockReturnValue({ id: "f2", name: "Personal" });
      render(ConfirmDialog);
      const { getByTitle, findByRole } = render(RepoRail, {
        props: baseProps({ onCreateFolder }),
      });

      await fireEvent.click(getByTitle("Create a new folder"));
      const dialog = within(await findByRole("alertdialog"));
      await fireEvent.input(dialog.getByRole("textbox"), { target: { value: "Personal" } });
      await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

      await waitFor(() => expect(onCreateFolder).toHaveBeenCalledWith("Personal"));
    });

    it("offers Move to <folder> and Remove from Recent from a repo's context menu", async () => {
      const onMoveRepo = vi.fn();
      const onRemoveRecent = vi.fn();
      render(ContextMenu);
      const { getByText, findByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo, onRemoveRecent }),
      });

      await fireEvent.contextMenu(getByText("ungrouped"));
      await fireEvent.click(await findByRole("menuitem", { name: 'Move to "Work"' }));

      expect(onMoveRepo).toHaveBeenCalledWith("/repo/ungrouped", "f1", null);

      await fireEvent.contextMenu(getByText("ungrouped"));
      await fireEvent.click(await findByRole("menuitem", { name: "Remove from Recent" }));

      expect(onRemoveRecent).toHaveBeenCalledWith("/repo/ungrouped");
    });

    it("offers Remove from Folder only for a repo already in one", async () => {
      const onMoveRepo = vi.fn();
      render(ContextMenu);
      const { getByText, findByRole, queryByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      await fireEvent.contextMenu(getByText("foldered"));
      expect(await findByRole("menuitem", { name: "Remove from Folder" })).toBeTruthy();
      expect(queryByRole("menuitem", { name: 'Move to "Work"' })).toBeNull();

      await fireEvent.click(await findByRole("menuitem", { name: "Remove from Folder" }));
      expect(onMoveRepo).toHaveBeenCalledWith("/repo/foldered", undefined, null);
    });

    it("creates a folder and files the repo into it via New Folder… on a repo's menu", async () => {
      const onCreateFolder = vi.fn().mockReturnValue({ id: "f2", name: "Personal" });
      const onMoveRepo = vi.fn();
      render(ConfirmDialog);
      render(ContextMenu);
      const { getByText, findByRole } = render(RepoRail, {
        props: baseProps({ recentRepos, onCreateFolder, onMoveRepo }),
      });

      await fireEvent.contextMenu(getByText("ungrouped"));
      await fireEvent.click(await findByRole("menuitem", { name: "New Folder…" }));
      const dialog = within(await findByRole("alertdialog"));
      await fireEvent.input(dialog.getByRole("textbox"), { target: { value: "Personal" } });
      await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

      await waitFor(() => expect(onCreateFolder).toHaveBeenCalledWith("Personal"));
      expect(onMoveRepo).toHaveBeenCalledWith("/repo/ungrouped", "f2", null);
    });

    it("renames a folder via its context menu", async () => {
      const onRenameFolder = vi.fn();
      render(ConfirmDialog);
      render(ContextMenu);
      const { getByText, findByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onRenameFolder }),
      });

      await fireEvent.contextMenu(getByText("Work"));
      await fireEvent.click(await findByRole("menuitem", { name: "Rename Folder…" }));
      const dialog = within(await findByRole("alertdialog"));
      expect((dialog.getByRole("textbox") as HTMLInputElement).value).toBe("Work");
      await fireEvent.input(dialog.getByRole("textbox"), { target: { value: "Client Work" } });
      await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

      await waitFor(() => expect(onRenameFolder).toHaveBeenCalledWith("f1", "Client Work"));
    });

    it("deletes a folder via its context menu after confirming", async () => {
      const onDeleteFolder = vi.fn();
      render(ConfirmDialog);
      render(ContextMenu);
      const { getByText, findByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onDeleteFolder }),
      });

      await fireEvent.contextMenu(getByText("Work"));
      await fireEvent.click(await findByRole("menuitem", { name: "Delete Folder" }));
      const dialog = within(await findByRole("alertdialog"));
      expect(dialog.getByText(/1 repo will move back to Recent/)).toBeTruthy();
      await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

      await waitFor(() => expect(onDeleteFolder).toHaveBeenCalledWith("f1"));
    });

    it("moves a repo into a folder when dropped on the folder header", () => {
      const onMoveRepo = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      const source = getByText("ungrouped").closest("li") as HTMLElement;
      const target = getByText("Work").closest(".folder-header") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      expect(onMoveRepo).toHaveBeenCalledWith("/repo/ungrouped", "f1", null);
    });

    it("highlights the folder header while a repo is dragged over it", async () => {
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos }),
      });

      const source = getByText("ungrouped").closest("li") as HTMLElement;
      const target = getByText("Work").closest(".folder-header") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      expect(target.className).not.toContain("drag-over");

      firePointer(source, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      await tick();
      expect(target.className).toContain("drag-over");

      firePointer(source, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });
      await tick();
      expect(target.className).not.toContain("drag-over");
    });

    it("reorders a repo when dropped onto another repo row", () => {
      const onMoveRepo = vi.fn();
      const twoUngrouped: RecentRepo[] = [
        { path: "/repo/a", lastOpenedAt: 2 },
        { path: "/repo/b", lastOpenedAt: 1 },
      ];
      const { getByText } = render(RepoRail, {
        props: baseProps({ recentRepos: twoUngrouped, onMoveRepo }),
      });

      const source = getByText("b").closest("li") as HTMLElement;
      const target = getByText("a").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: 20, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: 20, pointerId: 1 });

      expect(onMoveRepo).toHaveBeenCalledWith("/repo/b", undefined, "/repo/a");
    });

    it("un-files a repo when dropped on the Recent header", () => {
      const onMoveRepo = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      const source = getByText("foldered").closest("li") as HTMLElement;
      const target = getByText("Recent").closest(".recent-header") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: -20, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: -20, pointerId: 1 });

      expect(onMoveRepo).toHaveBeenCalledWith("/repo/foldered", undefined, null);
    });

    it("reorders folders when one is dropped onto another", () => {
      const onMoveFolder = vi.fn();
      const twoFolders: RepoFolder[] = [
        { id: "f1", name: "Work" },
        { id: "f2", name: "Personal" },
      ];
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders: twoFolders, onMoveFolder }),
      });

      const source = getByText("Personal").closest(".folder-header") as HTMLElement;
      const target = getByText("Work").closest(".folder-header") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: 20, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: 20, pointerId: 1 });

      expect(onMoveFolder).toHaveBeenCalledWith("f2", "f1");
    });

    it("does not move a repo on a plain click (movement below the drag threshold)", () => {
      const onMoveRepo = vi.fn();
      const onSelectRecent = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo, onSelectRecent }),
      });

      const source = getByText("ungrouped").closest("li") as HTMLElement;
      const target = getByText("Work").closest(".folder-header") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);

      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: 0, pointerId: 1 });

      expect(onMoveRepo).not.toHaveBeenCalled();
    });

    it("still selects a repo on click even after jitter that crosses the drag threshold but releases back over itself", async () => {
      const onSelectRecent = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ recentRepos, onSelectRecent }),
      });

      const row = getByText("ungrouped").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(row);

      firePointer(row, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(row, "pointermove", { clientX: 8, clientY: 0, pointerId: 1 });
      firePointer(row, "pointerup", { clientX: 8, clientY: 0, pointerId: 1 });
      await fireEvent.click(getByText("ungrouped"));

      expect(onSelectRecent).toHaveBeenCalledWith("/repo/ungrouped");
    });

    it("still removes a repo on click even after jitter that crosses the drag threshold", async () => {
      const onRemoveRecent = vi.fn();
      const { getByText, getByTitle } = render(RepoRail, {
        props: baseProps({ recentRepos, onRemoveRecent }),
      });

      const row = getByText("ungrouped").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(row);

      firePointer(row, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(row, "pointermove", { clientX: 0, clientY: 8, pointerId: 1 });
      firePointer(row, "pointerup", { clientX: 0, clientY: 8, pointerId: 1 });
      await fireEvent.click(getByTitle("Remove /repo/ungrouped from recent"));

      expect(onRemoveRecent).toHaveBeenCalledWith("/repo/ungrouped");
    });
  });

  describe("notifications", () => {
    beforeEach(() => {
      toastState.toasts = [];
      notificationHistory.entries = [];
    });

    it("shows a Notifications button above Settings, with no unread badge initially", () => {
      const { getByRole, queryByText } = render(RepoRail, { props: baseProps() });

      const notificationsButton = getByRole("button", { name: /Notifications/ });
      const settingsButton = getByRole("button", { name: "Settings" });
      expect(
        notificationsButton.compareDocumentPosition(settingsButton) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(queryByText("1")).toBeNull();
    });

    it("shows an unread count badge that updates as notifications are marked viewed", async () => {
      notifySuccess("Fetched from origin.");
      notifyError("Push rejected.");

      const { getByRole, getByText, findAllByTitle, queryByText } = render(RepoRail, {
        props: baseProps(),
      });

      expect(getByText("2")).toBeTruthy();

      await fireEvent.click(getByRole("button", { name: /Notifications/ }));
      const [firstMarkViewed] = await findAllByTitle("Mark as viewed");
      await fireEvent.click(firstMarkViewed);

      expect(getByText("1")).toBeTruthy();
      expect(queryByText("2")).toBeNull();
    });

    it("opens the notification history panel on click", async () => {
      notifySuccess("Fetched from origin.");

      const { getByRole, findByText } = render(RepoRail, { props: baseProps() });

      await fireEvent.click(getByRole("button", { name: /Notifications/ }));

      expect(await findByText("Fetched from origin.")).toBeTruthy();
    });
  });
});
