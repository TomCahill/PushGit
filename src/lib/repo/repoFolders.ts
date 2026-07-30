// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Folders for organizing opened repos in the left rail, persisted to localStorage —
// mirrors `recentRepos.ts`'s style (plain functions, array position *is* display order, no
// separate numeric `order` field). A `RecentRepo` (`./recentRepos.ts`) points at a folder via
// its own `folderId`; this module only owns the folders themselves.
const STORAGE_KEY = "pushgit.repoFolders";

export interface RepoFolder {
  id: string;
  name: string;
  collapsed?: boolean;
}

function isRepoFolder(value: unknown): value is RepoFolder {
  const candidate = value as Partial<RepoFolder> | null;
  return (
    typeof candidate?.id === "string" &&
    candidate.id.length > 0 &&
    typeof candidate.name === "string" &&
    candidate.name.length > 0 &&
    (candidate.collapsed === undefined || typeof candidate.collapsed === "boolean")
  );
}

export function loadRepoFolders(): RepoFolder[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isRepoFolder) : [];
  } catch {
    return [];
  }
}

function save(folders: RepoFolder[]): RepoFolder[] {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(folders));
  return folders;
}

/** Appends a new folder and returns both the updated list and the created folder, since callers
 *  (e.g. "New Folder…" from a repo's context menu) often need the fresh id immediately. */
export function createFolder(name: string): { folders: RepoFolder[]; folder: RepoFolder } {
  const folder: RepoFolder = { id: crypto.randomUUID(), name };
  const folders = save([...loadRepoFolders(), folder]);
  return { folders, folder };
}

export function renameFolder(id: string, name: string): RepoFolder[] {
  return save(loadRepoFolders().map((folder) => (folder.id === id ? { ...folder, name } : folder)));
}

export function deleteFolder(id: string): RepoFolder[] {
  return save(loadRepoFolders().filter((folder) => folder.id !== id));
}

export function toggleFolderCollapsed(id: string): RepoFolder[] {
  return save(
    loadRepoFolders().map((folder) =>
      folder.id === id ? { ...folder, collapsed: !folder.collapsed } : folder,
    ),
  );
}

/** Repositions folder `id` immediately before `beforeId` if given and present, otherwise at the
 *  end — same splice-before-target approach as `recentRepos.ts`'s `moveRepo`. */
export function moveFolder(id: string, beforeId: string | null): RepoFolder[] {
  const current = loadRepoFolders();
  const folder = current.find((f) => f.id === id);
  if (!folder) return current;

  const rest = current.filter((f) => f.id !== id);
  const insertAt = beforeId ? rest.findIndex((f) => f.id === beforeId) : -1;
  const next =
    insertAt === -1
      ? [...rest, folder]
      : [...rest.slice(0, insertAt), folder, ...rest.slice(insertAt)];
  return save(next);
}
