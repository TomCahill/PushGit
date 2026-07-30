// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Recently-opened repo paths, most-recent-first, capped and persisted to localStorage —
// the left rail's job is now "switch repos", not "act on the current one" (`ActionRail.svelte`
// covers that), so it needs more than the single last-opened path `+page.svelte` used to keep.
const STORAGE_KEY = "pushgit.recentRepos";
const MAX_ENTRIES = 8;

export interface RecentRepo {
  path: string;
  lastOpenedAt: number;
  /** Folder this repo is filed into, if any. Absent/undefined means ungrouped ("Recent"). */
  folderId?: string;
}

function isRecentRepo(value: unknown): value is RecentRepo {
  const candidate = value as Partial<RecentRepo> | null;
  return (
    typeof candidate?.path === "string" &&
    candidate.path.length > 0 &&
    typeof candidate.lastOpenedAt === "number" &&
    (candidate.folderId === undefined || typeof candidate.folderId === "string")
  );
}

export function loadRecentRepos(): RecentRepo[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isRecentRepo) : [];
  } catch {
    return [];
  }
}

function save(entries: RecentRepo[]): RecentRepo[] {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(entries));
  return entries;
}

/** Moves `path` to the front (deduped, preserving its `folderId` if it had one) with a fresh
 *  timestamp. Only trims ungrouped entries down to `MAX_ENTRIES` — a repo the user has filed
 *  into a folder was deliberately kept, so it's never silently evicted by the cap. */
export function recordRepoOpened(path: string): RecentRepo[] {
  const previous = loadRecentRepos();
  const existing = previous.find((entry) => entry.path === path);
  const rest = previous.filter((entry) => entry.path !== path);
  const updated: RecentRepo[] = [
    { path, lastOpenedAt: Date.now(), folderId: existing?.folderId },
    ...rest,
  ];
  const overflow = new Set(
    updated
      .filter((entry) => !entry.folderId)
      .slice(MAX_ENTRIES)
      .map((entry) => entry.path),
  );
  return save(updated.filter((entry) => !overflow.has(entry.path)));
}

export function removeRecentRepo(path: string): RecentRepo[] {
  return save(loadRecentRepos().filter((entry) => entry.path !== path));
}

/** Assigns `path` to `folderId` (`undefined` un-files it back to "Recent") and repositions it
 *  immediately before `beforePath` if given and present, otherwise at the end. Covers
 *  assign-to-folder, un-file, and manual same-group reorder — they're all just "move this entry
 *  to a folder and a position". */
export function moveRepo(
  path: string,
  folderId: string | undefined,
  beforePath: string | null,
): RecentRepo[] {
  const current = loadRecentRepos();
  const entry = current.find((e) => e.path === path);
  if (!entry) return current;

  const rest = current.filter((e) => e.path !== path);
  const moved: RecentRepo = { ...entry, folderId };
  const insertAt = beforePath ? rest.findIndex((e) => e.path === beforePath) : -1;
  const next =
    insertAt === -1
      ? [...rest, moved]
      : [...rest.slice(0, insertAt), moved, ...rest.slice(insertAt)];
  return save(next);
}

/** Strips `folderId` from any repo referencing a deleted folder, sending them back to "Recent". */
export function unassignReposFromFolder(folderId: string): RecentRepo[] {
  const current = loadRecentRepos();
  return save(
    current.map((entry) =>
      entry.folderId === folderId ? { path: entry.path, lastOpenedAt: entry.lastOpenedAt } : entry,
    ),
  );
}

/** The last path segment, for a compact display name — falls back to the full path for a
 *  root-only path like `/` or `C:\`. */
export function repoDisplayName(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, "");
  const segment = trimmed.split(/[/\\]/).pop();
  return segment || path;
}
