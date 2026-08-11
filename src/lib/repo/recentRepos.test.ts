// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import {
  loadRecentRepos,
  moveRepo,
  recordRepoOpened,
  removeRecentRepo,
  repoDisplayName,
  unassignReposFromFolder,
} from "./recentRepos";

afterEach(() => {
  localStorage.clear();
});

describe("recentRepos", () => {
  it("starts empty", () => {
    expect(loadRecentRepos()).toEqual([]);
  });

  it("records an opened repo and persists it", () => {
    recordRepoOpened("/repo/one");

    const loaded = loadRecentRepos();
    expect(loaded).toHaveLength(1);
    expect(loaded[0].path).toBe("/repo/one");
  });

  it("moves a re-opened repo to the front instead of duplicating it", () => {
    recordRepoOpened("/repo/one");
    recordRepoOpened("/repo/two");
    recordRepoOpened("/repo/one");

    const loaded = loadRecentRepos();
    expect(loaded.map((e) => e.path)).toEqual(["/repo/one", "/repo/two"]);
  });

  it("caps the list at 8 entries, dropping the oldest", () => {
    for (let i = 0; i < 10; i++) {
      recordRepoOpened(`/repo/${i}`);
    }

    const loaded = loadRecentRepos();
    expect(loaded).toHaveLength(8);
    expect(loaded.map((e) => e.path)).toEqual([
      "/repo/9",
      "/repo/8",
      "/repo/7",
      "/repo/6",
      "/repo/5",
      "/repo/4",
      "/repo/3",
      "/repo/2",
    ]);
  });

  it("removes a repo from the list", () => {
    recordRepoOpened("/repo/one");
    recordRepoOpened("/repo/two");

    removeRecentRepo("/repo/one");

    expect(loadRecentRepos().map((e) => e.path)).toEqual(["/repo/two"]);
  });

  it("ignores malformed JSON already in storage", () => {
    localStorage.setItem("pushgit.recentRepos", "{not json");
    expect(loadRecentRepos()).toEqual([]);
  });

  it("ignores non-array or malformed entries already in storage", () => {
    localStorage.setItem(
      "pushgit.recentRepos",
      JSON.stringify([{ path: "/ok", lastOpenedAt: 1 }, { path: 123 }, "nonsense"]),
    );
    expect(loadRecentRepos()).toEqual([{ path: "/ok", lastOpenedAt: 1 }]);
  });

  it("preserves folderId and position when a foldered repo is re-opened", () => {
    recordRepoOpened("/repo/one");
    recordRepoOpened("/repo/two");
    moveRepo("/repo/one", "folder-1", null);

    const before = loadRecentRepos().find((e) => e.path === "/repo/one");
    recordRepoOpened("/repo/one");

    const loaded = loadRecentRepos();
    expect(loaded.map((e) => e.path)).toEqual(["/repo/two", "/repo/one"]);
    const after = loaded.find((e) => e.path === "/repo/one");
    expect(after?.folderId).toBe("folder-1");
    expect(after?.lastOpenedAt).toBeGreaterThanOrEqual(before?.lastOpenedAt ?? 0);
  });

  it("does not reorder foldered repos relative to each other on reopen", () => {
    recordRepoOpened("/repo/a");
    recordRepoOpened("/repo/b");
    moveRepo("/repo/a", "folder-1", null);
    moveRepo("/repo/b", "folder-1", null); // order within folder-1: a, b

    recordRepoOpened("/repo/a");

    const inFolder = loadRecentRepos().filter((e) => e.folderId === "folder-1");
    expect(inFolder.map((e) => e.path)).toEqual(["/repo/a", "/repo/b"]);
  });

  it("never evicts foldered repos, only unfoldered ones, once over the cap", () => {
    recordRepoOpened("/repo/pinned");
    moveRepo("/repo/pinned", "folder-1", null);

    for (let i = 0; i < 10; i++) {
      recordRepoOpened(`/repo/${i}`);
    }

    const loaded = loadRecentRepos();
    expect(loaded.find((e) => e.path === "/repo/pinned")?.folderId).toBe("folder-1");
    expect(loaded.filter((e) => !e.folderId)).toHaveLength(8);
    expect(loaded).toHaveLength(9);
  });

  describe("moveRepo", () => {
    it("assigns a repo to a folder", () => {
      recordRepoOpened("/repo/one");

      moveRepo("/repo/one", "folder-1", null);

      expect(loadRecentRepos()[0].folderId).toBe("folder-1");
    });

    it("un-files a repo when folderId is undefined", () => {
      recordRepoOpened("/repo/one");
      moveRepo("/repo/one", "folder-1", null);

      moveRepo("/repo/one", undefined, null);

      expect(loadRecentRepos()[0].folderId).toBeUndefined();
    });

    it("repositions a repo immediately before beforePath", () => {
      recordRepoOpened("/repo/a");
      recordRepoOpened("/repo/b");
      recordRepoOpened("/repo/c"); // order: c, b, a

      moveRepo("/repo/a", undefined, "/repo/b");

      expect(loadRecentRepos().map((e) => e.path)).toEqual(["/repo/c", "/repo/a", "/repo/b"]);
    });

    it("appends to the end when beforePath is null", () => {
      recordRepoOpened("/repo/a");
      recordRepoOpened("/repo/b"); // order: b, a

      moveRepo("/repo/a", undefined, null);

      expect(loadRecentRepos().map((e) => e.path)).toEqual(["/repo/b", "/repo/a"]);
    });

    it("is a no-op for an unknown path", () => {
      recordRepoOpened("/repo/a");

      const result = moveRepo("/repo/missing", "folder-1", null);

      expect(result.map((e) => e.path)).toEqual(["/repo/a"]);
    });
  });

  describe("unassignReposFromFolder", () => {
    it("strips folderId from repos referencing a deleted folder", () => {
      recordRepoOpened("/repo/one");
      recordRepoOpened("/repo/two");
      moveRepo("/repo/one", "folder-1", null);
      moveRepo("/repo/two", "folder-2", null);

      unassignReposFromFolder("folder-1");

      const loaded = loadRecentRepos();
      expect(loaded.find((e) => e.path === "/repo/one")?.folderId).toBeUndefined();
      expect(loaded.find((e) => e.path === "/repo/two")?.folderId).toBe("folder-2");
    });
  });
});

describe("repoDisplayName", () => {
  it("returns the last path segment", () => {
    expect(repoDisplayName("/home/tom/Projects/pushgit")).toBe("pushgit");
  });

  it("handles a trailing slash", () => {
    expect(repoDisplayName("/home/tom/Projects/pushgit/")).toBe("pushgit");
  });

  it("handles Windows-style backslash paths", () => {
    expect(repoDisplayName("C:\\Users\\tom\\pushgit")).toBe("pushgit");
  });

  it("falls back to the full path when there's no segment to extract", () => {
    expect(repoDisplayName("/")).toBe("/");
  });
});
