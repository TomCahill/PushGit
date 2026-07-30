// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import {
  createFolder,
  deleteFolder,
  loadRepoFolders,
  moveFolder,
  renameFolder,
  toggleFolderCollapsed,
} from "./repoFolders";

afterEach(() => {
  localStorage.clear();
});

describe("repoFolders", () => {
  it("starts empty", () => {
    expect(loadRepoFolders()).toEqual([]);
  });

  it("creates a folder and persists it", () => {
    const { folders, folder } = createFolder("Work");

    expect(folder.name).toBe("Work");
    expect(folder.id).toBeTruthy();
    expect(folders).toEqual([folder]);
    expect(loadRepoFolders()).toEqual([folder]);
  });

  it("appends new folders to the end", () => {
    const { folder: first } = createFolder("Work");
    const { folders } = createFolder("Personal");

    expect(folders.map((f) => f.id)).toEqual([first.id, folders[1].id]);
    expect(folders.map((f) => f.name)).toEqual(["Work", "Personal"]);
  });

  it("renames a folder", () => {
    const { folder } = createFolder("Work");

    renameFolder(folder.id, "Client Work");

    expect(loadRepoFolders()).toEqual([{ id: folder.id, name: "Client Work" }]);
  });

  it("deletes a folder", () => {
    const { folder: keep } = createFolder("Work");
    const { folder: gone } = createFolder("Personal");

    deleteFolder(gone.id);

    expect(loadRepoFolders()).toEqual([keep]);
  });

  it("toggles a folder's collapsed state", () => {
    const { folder } = createFolder("Work");
    expect(folder.collapsed).toBeUndefined();

    toggleFolderCollapsed(folder.id);
    expect(loadRepoFolders()[0].collapsed).toBe(true);

    toggleFolderCollapsed(folder.id);
    expect(loadRepoFolders()[0].collapsed).toBe(false);
  });

  it("reorders a folder before another", () => {
    const { folder: work } = createFolder("Work");
    const { folder: personal } = createFolder("Personal");
    createFolder("Archive");

    moveFolder("archive-missing-noop", personal.id); // no-op for an unknown id
    moveFolder(loadRepoFolders()[2].id, work.id); // move "Archive" before "Work"

    expect(loadRepoFolders().map((f) => f.name)).toEqual(["Archive", "Work", "Personal"]);
  });

  it("moves a folder to the end when beforeId is null", () => {
    const { folder: work } = createFolder("Work");
    createFolder("Personal");

    moveFolder(work.id, null);

    expect(loadRepoFolders().map((f) => f.name)).toEqual(["Personal", "Work"]);
  });

  it("ignores malformed JSON already in storage", () => {
    localStorage.setItem("pushgit.repoFolders", "{not json");
    expect(loadRepoFolders()).toEqual([]);
  });

  it("ignores non-array or malformed entries already in storage", () => {
    localStorage.setItem(
      "pushgit.repoFolders",
      JSON.stringify([{ id: "1", name: "ok" }, { id: "2" }, "nonsense"]),
    );
    expect(loadRepoFolders()).toEqual([{ id: "1", name: "ok" }]);
  });
});
