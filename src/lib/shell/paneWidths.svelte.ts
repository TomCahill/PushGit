// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Persisted widths for the two draggable dividers in the 3-pane shell (`+page.svelte`) —
// the center column always fills whatever space remains (`1fr`), shrinking/growing as the
// window is resized, so only the repo rail and sidebar (diff/staging) columns need a stored
// width. Initialized from localStorage synchronously at module load, matching
// `settings.svelte.ts`'s pattern, so the shell never flashes at the default width before a
// saved one loads in.
const STORAGE_KEY = "pushgit.paneWidths";

export const REPO_RAIL_MIN = 160;
export const REPO_RAIL_MAX = 480;
export const REPO_RAIL_DEFAULT = 220;
// Floor for the center column's flexible track (`minmax(CENTER_MIN, 1fr)`) — not persisted,
// since the center column itself isn't a fixed width.
export const CENTER_MIN = 280;
export const SIDEBAR_MIN = 280;
export const SIDEBAR_MAX = 1200;
export const SIDEBAR_DEFAULT = 480;

interface PersistedPaneWidths {
  repoRail: number;
  sidebar: number;
}

function isValidWidth(value: unknown, min: number, max: number): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= min && value <= max;
}

/** Pure parsing/validation, split out from the module-load-time read so it's testable
 *  without fighting this module's own localStorage read at import time. */
export function parsePaneWidthsJson(raw: string | null): PersistedPaneWidths {
  try {
    if (!raw) return { repoRail: REPO_RAIL_DEFAULT, sidebar: SIDEBAR_DEFAULT };
    const parsed: unknown = JSON.parse(raw);
    const candidate = parsed as Partial<PersistedPaneWidths> | null;
    return {
      repoRail: isValidWidth(candidate?.repoRail, REPO_RAIL_MIN, REPO_RAIL_MAX)
        ? candidate.repoRail
        : REPO_RAIL_DEFAULT,
      sidebar: isValidWidth(candidate?.sidebar, SIDEBAR_MIN, SIDEBAR_MAX)
        ? candidate.sidebar
        : SIDEBAR_DEFAULT,
    };
  } catch {
    return { repoRail: REPO_RAIL_DEFAULT, sidebar: SIDEBAR_DEFAULT };
  }
}

export const paneWidthsState: PersistedPaneWidths = $state(
  parsePaneWidthsJson(localStorage.getItem(STORAGE_KEY)),
);

function persist(): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(paneWidthsState));
}

export function setRepoRailWidth(value: number): void {
  paneWidthsState.repoRail = Math.min(REPO_RAIL_MAX, Math.max(REPO_RAIL_MIN, value));
  persist();
}

export function setSidebarWidth(value: number): void {
  paneWidthsState.sidebar = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, value));
  persist();
}
