// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import ContextMenu from "$lib/shell/ContextMenu.svelte";
import { firePointer } from "$lib/shell/testPointerEvents";
import {
  notificationHistory,
  notifyError,
  notifySuccess,
  toastState,
} from "$lib/shell/toast.svelte";
import { updateCheckState } from "$lib/shell/updateCheck.svelte";
import RepoRail from "./RepoRail.svelte";
import type { RecentRepo } from "./recentRepos";
import type { RepoFolder } from "./repoFolders";

// Folder names also appear as labels on Recent rows, so target the header by its button role.
function folderHeader(name: string): HTMLElement {
  return screen.getByRole("button", { name: new RegExp(`^${name}\\b`) });
}

function inFolder(name: string) {
  return within(folderHeader(name).closest(".folder") as HTMLElement);
}

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
    onCollapseAllFolders: vi.fn(),
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

  it("shows the parent directory only for repos whose names clash", () => {
    const { getByTitle } = render(RepoRail, {
      props: baseProps({
        recentRepos: [
          { path: "/p/Lighten/api", lastOpenedAt: 3 },
          { path: "/p/Buttress/api", lastOpenedAt: 2 },
          { path: "/p/Buttress/infra", lastOpenedAt: 1 },
        ],
      }),
    });

    expect(getByTitle("/p/Lighten/api").textContent).toContain("Lighten");
    expect(getByTitle("/p/Buttress/api").textContent).toContain("Buttress");
    expect(getByTitle("/p/Buttress/infra").textContent).not.toContain("Buttress");
  });

  it("doesn't repeat the parent directory when it matches the row's folder label", async () => {
    const { getByLabelText, getByTitle } = render(RepoRail, {
      props: baseProps({
        folders: [{ id: "f1", name: "Lighten" }],
        recentRepos: [
          { path: "/p/Lighten/api", lastOpenedAt: 2, folderId: "f1" },
          { path: "/p/Buttress/api", lastOpenedAt: 1 },
        ],
      }),
    });

    await fireEvent.input(getByLabelText("Filter repositories"), { target: { value: "api" } });

    expect(getByTitle("/p/Lighten/api").textContent?.match(/Lighten/g)).toHaveLength(1);
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

    await fireEvent.click(getByTitle("Remove /home/tom/Projects/pushgit from list"));

    expect(onRemoveRecent).toHaveBeenCalledWith("/home/tom/Projects/pushgit");
    expect(onSelectRecent).not.toHaveBeenCalled();
  });

  it("labels the Open button only until a repo has been opened", async () => {
    const { getByTitle, rerender } = render(RepoRail, { props: baseProps() });
    expect(getByTitle("Open a repository").textContent).toContain("Open");

    await rerender(baseProps({ recentRepos: [{ path: "/repo/a", lastOpenedAt: 1 }] }));

    expect(getByTitle("Open a repository").textContent?.trim()).toBe("");
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
    const { getByRole, getByText } = render(RepoRail, {
      props: baseProps({
        currentPath: "/home/tom/Projects/pushgit",
        recentRepos: [
          { path: "/home/tom/Projects/pushgit", lastOpenedAt: 2 },
          { path: "/home/tom/Projects/other-repo", lastOpenedAt: 1 },
        ],
      }),
    });

    const currentRow = getByRole("button", { name: "pushgit" }).closest("li");
    const otherRow = getByText("other-repo").closest("li");
    expect(currentRow?.className).toContain("current");
    expect(otherRow?.className).not.toContain("current");
    expect(document.querySelector(".sidebar-footer")?.textContent).toContain("pushgit");
  });

  describe("folders", () => {
    const folders: RepoFolder[] = [{ id: "f1", name: "Work" }];
    const recentRepos: RecentRepo[] = [
      { path: "/repo/foldered", lastOpenedAt: 2, folderId: "f1" },
      { path: "/repo/ungrouped", lastOpenedAt: 1 },
    ];

    it("groups repos under their folder, separate from the ungrouped Recent list", () => {
      const { getByText } = render(RepoRail, { props: baseProps({ folders, recentRepos }) });

      expect(folderHeader("Work")).toBeTruthy();
      expect(inFolder("Work").getByText("foldered")).toBeTruthy();
      expect(getByText("ungrouped")).toBeTruthy();
      expect(getByText("1")).toBeTruthy(); // folder's repo count
    });

    it("collapses every folder from the toolbar", async () => {
      const onCollapseAllFolders = vi.fn();
      const { getByTitle } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onCollapseAllFolders }),
      });

      await fireEvent.click(getByTitle("Collapse all folders"));

      expect(onCollapseAllFolders).toHaveBeenCalled();
    });

    it("marks a collapsed folder that holds the open repo", () => {
      const props = { folders: [{ id: "f1", name: "Work", collapsed: true }], recentRepos };
      const { queryByTitle, unmount } = render(RepoRail, {
        props: baseProps({ ...props, currentPath: "/repo/foldered" }),
      });
      expect(queryByTitle("The open repository is in this folder")).toBeTruthy();
      unmount();

      const other = render(RepoRail, {
        props: baseProps({ ...props, currentPath: "/repo/ungrouped" }),
      });
      expect(other.queryByTitle("The open repository is in this folder")).toBeNull();
    });

    it("toggles a folder collapsed when its header is clicked", async () => {
      const onToggleFolderCollapsed = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onToggleFolderCollapsed }),
      });

      await fireEvent.click(folderHeader("Work"));

      expect(onToggleFolderCollapsed).toHaveBeenCalledWith("f1");
    });

    it("hides a folder's repos when collapsed", () => {
      const { getByText, queryByText } = render(RepoRail, {
        props: baseProps({ folders: [{ id: "f1", name: "Work", collapsed: true }], recentRepos }),
      });

      expect(folderHeader("Work")).toBeTruthy();
      expect(inFolder("Work").queryByText("foldered")).toBeNull();
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

    it("offers Move to Folder… and Remove from list from a repo's context menu", async () => {
      const onMoveRepo = vi.fn();
      const onRemoveRecent = vi.fn();
      render(ContextMenu);
      const { getByText, findByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo, onRemoveRecent }),
      });

      await fireEvent.contextMenu(getByText("ungrouped"));
      await fireEvent.click(await findByRole("menuitem", { name: "Move to Folder…" }));
      await fireEvent.click(await findByRole("menuitem", { name: "Work" }));

      expect(onMoveRepo).toHaveBeenCalledWith("/repo/ungrouped", "f1", null);

      await fireEvent.contextMenu(getByText("ungrouped"));
      await fireEvent.click(await findByRole("menuitem", { name: "Remove from list" }));

      expect(onRemoveRecent).toHaveBeenCalledWith("/repo/ungrouped");
    });

    it("offers Remove from Folder for a filed repo, and doesn't offer its own folder as a move target", async () => {
      const onMoveRepo = vi.fn();
      render(ContextMenu);
      const { getByText, findByRole, queryByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      await fireEvent.contextMenu(inFolder("Work").getByText("foldered"));
      await fireEvent.click(await findByRole("menuitem", { name: "Remove from Folder" }));
      expect(onMoveRepo).toHaveBeenCalledWith("/repo/foldered", undefined, null);

      await fireEvent.contextMenu(inFolder("Work").getByText("foldered"));
      await fireEvent.click(await findByRole("menuitem", { name: "Move to Folder…" }));
      expect(await findByRole("menuitem", { name: "New Folder…" })).toBeTruthy();
      expect(queryByRole("menuitem", { name: "Work" })).toBeNull();
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
      await fireEvent.click(await findByRole("menuitem", { name: "Move to Folder…" }));
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

      await fireEvent.contextMenu(folderHeader("Work"));
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

      await fireEvent.contextMenu(folderHeader("Work"));
      await fireEvent.click(await findByRole("menuitem", { name: "Delete Folder" }));
      const dialog = within(await findByRole("alertdialog"));
      expect(dialog.getByText(/1 repo will move back to Unsorted/)).toBeTruthy();
      await fireEvent.click(dialog.getByRole("button", { name: "Delete folder" }));

      await waitFor(() => expect(onDeleteFolder).toHaveBeenCalledWith("f1"));
    });

    it("moves a repo into a folder when dropped on the folder header", () => {
      const onMoveRepo = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      const source = getByText("ungrouped").closest("li") as HTMLElement;
      const target = folderHeader("Work").closest(".folder-header") as HTMLElement;
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
      const target = folderHeader("Work").closest(".folder-header") as HTMLElement;
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

    it("un-files a repo when dropped on the Unsorted header", () => {
      const onMoveRepo = vi.fn();
      const { getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      const source = inFolder("Work").getByText("foldered").closest("li") as HTMLElement;
      const target = getByText("Unsorted").closest(".recent-header") as HTMLElement;
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
      const target = folderHeader("Work").closest(".folder-header") as HTMLElement;
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
      const target = folderHeader("Work").closest(".folder-header") as HTMLElement;
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
      await fireEvent.click(getByTitle("Remove /repo/ungrouped from list"));

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

  describe("update banner", () => {
    beforeEach(() => {
      updateCheckState.available = null;
    });

    it("shows nothing when there's no update available", () => {
      const { queryByText } = render(RepoRail, { props: baseProps() });

      expect(queryByText(/available/)).toBeNull();
    });

    it("opens the release page in the system browser when clicked", async () => {
      updateCheckState.available = {
        version: "1.3.0",
        url: "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0",
        publishedAt: "2026-08-01T00:00:00Z",
      };
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "plugin:opener|open_url") {
          calls.push(args);
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByText } = render(RepoRail, { props: baseProps() });
      await fireEvent.click(getByText("v1.3.0 available"));

      expect(calls).toEqual([
        { url: "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0", openWith: undefined },
      ]);
    });

    it("dismisses the banner and persists the dismissal", async () => {
      updateCheckState.available = {
        version: "1.3.0",
        url: "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0",
        publishedAt: "2026-08-01T00:00:00Z",
      };
      mockIPC((cmd, args) => {
        if (cmd === "dismiss_update") {
          expect(args).toEqual({ version: "1.3.0" });
          return { dismissedUpdateVersion: "1.3.0" };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByTitle, queryByText } = render(RepoRail, { props: baseProps() });
      await fireEvent.click(getByTitle("Dismiss"));
      await tick();

      expect(queryByText("v1.3.0 available")).toBeNull();
    });
  });

  describe("filter", () => {
    const folders: RepoFolder[] = [{ id: "f1", name: "Work", collapsed: true }];
    const recentRepos: RecentRepo[] = [
      { path: "/repo/api-server", lastOpenedAt: 3, folderId: "f1" },
      { path: "/repo/web-app", lastOpenedAt: 2 },
      { path: "/repo/api-client", lastOpenedAt: 1 },
    ];

    async function typeFilter(input: HTMLElement, value: string) {
      await fireEvent.input(input, { target: { value } });
    }

    it("matches repos inside collapsed folders and labels them with their folder", async () => {
      const { getByLabelText, getByText, queryByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos }),
      });

      await typeFilter(getByLabelText("Filter repositories"), "API");

      expect(getByText("api-server")).toBeTruthy();
      expect(getByText("api-client")).toBeTruthy();
      expect(queryByText("web-app")).toBeNull();
      expect(getByText("api-server").closest("button")?.textContent).toContain("Work");
    });

    it("opens the active match on Enter, moving it with the arrow keys, then clears", async () => {
      const onSelectRecent = vi.fn();
      const { getByLabelText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onSelectRecent }),
      });
      const input = getByLabelText("Filter repositories") as HTMLInputElement;

      await typeFilter(input, "api");
      await fireEvent.keyDown(input, { key: "ArrowDown" });
      await fireEvent.keyDown(input, { key: "Enter" });

      expect(onSelectRecent).toHaveBeenCalledWith("/repo/api-client");
      expect(input.value).toBe("");
    });

    it("clears on Escape and restores the folder view", async () => {
      const { getByLabelText, getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos }),
      });
      const input = getByLabelText("Filter repositories") as HTMLInputElement;

      await typeFilter(input, "api");
      await fireEvent.keyDown(input, { key: "Escape" });

      expect(input.value).toBe("");
      expect(folderHeader("Work")).toBeTruthy();
      expect(getByText("web-app")).toBeTruthy();
    });

    it("says so when nothing matches", async () => {
      const { getByLabelText, getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos }),
      });

      await typeFilter(getByLabelText("Filter repositories"), "nope");

      expect(getByText('No repos match "nope".')).toBeTruthy();
    });

    it("doesn't start a drag from a filtered row", async () => {
      const onMoveRepo = vi.fn();
      const { getByLabelText, getByText } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });
      await typeFilter(getByLabelText("Filter repositories"), "api");

      const source = getByText("api-client").closest("li") as HTMLElement;
      const target = getByText("api-server").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(target);
      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: -20, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: -20, pointerId: 1 });

      expect(onMoveRepo).not.toHaveBeenCalled();
    });
  });

  describe("recent and unsorted", () => {
    const folders: RepoFolder[] = [
      { id: "f1", name: "Work", collapsed: true },
      { id: "f2", name: "Home", collapsed: true },
    ];
    const recentRepos: RecentRepo[] = [
      { path: "/repo/oldest", lastOpenedAt: 1, folderId: "f1" },
      { path: "/repo/newest", lastOpenedAt: 5, folderId: "f2" },
      { path: "/repo/middle", lastOpenedAt: 3, folderId: "f1" },
      { path: "/repo/second", lastOpenedAt: 4, folderId: "f1" },
      { path: "/repo/loose", lastOpenedAt: 9 },
    ];

    it("lists the three most recently opened foldered repos, with their folder", () => {
      const { getByRole } = render(RepoRail, { props: baseProps({ folders, recentRepos }) });

      const recent = getByRole("list", { name: "Recently opened repositories" });
      const names = [...recent.querySelectorAll(".recent-name")].map((el) => el.textContent);
      expect(names).toEqual(["newest", "second", "middle"]);
      expect(within(recent).getByText("Home")).toBeTruthy();
    });

    it("doesn't drag Recent rows", () => {
      const onMoveRepo = vi.fn();
      const { getByRole } = render(RepoRail, {
        props: baseProps({ folders, recentRepos, onMoveRepo }),
      });

      const recent = getByRole("list", { name: "Recently opened repositories" });
      const source = within(recent).getByText("newest").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(folderHeader("Work"));
      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: 40, pointerId: 1 });
      firePointer(source, "pointerup", { clientX: 0, clientY: 40, pointerId: 1 });

      expect(onMoveRepo).not.toHaveBeenCalled();
    });

    it("hides an empty Unsorted header until a drag starts", async () => {
      const filed = recentRepos.filter((entry) => entry.folderId);
      const { queryByText } = render(RepoRail, {
        props: baseProps({ folders: [{ id: "f1", name: "Work" }, folders[1]], recentRepos: filed }),
      });
      expect(queryByText("Unsorted")).toBeNull();

      const source = inFolder("Work").getByText("oldest").closest("li") as HTMLElement;
      vi.spyOn(document, "elementFromPoint").mockReturnValue(source);
      firePointer(source, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(source, "pointermove", { clientX: 0, clientY: 20, pointerId: 1 });
      await tick();

      expect(queryByText("Unsorted")).toBeTruthy();
    });

    it("labels ungrouped repos Repositories when there are no folders", () => {
      const { getByText, queryByText } = render(RepoRail, {
        props: baseProps({ recentRepos: [{ path: "/repo/loose", lastOpenedAt: 1 }] }),
      });

      expect(getByText("Repositories")).toBeTruthy();
      expect(queryByText("Recent")).toBeNull();
    });
  });
});
