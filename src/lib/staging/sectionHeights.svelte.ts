// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Persisted heights for the two draggable dividers stacked in `StagingPanel.svelte` — the
// commit box always fills whatever space remains below the staged section, so only the
// unstaged and staged sections need a stored height. Mirrors `shell/paneWidths.svelte.ts`:
// initialized from localStorage synchronously at module load so the panel never flashes at the
// default size before a saved one loads in.
const STORAGE_KEY = "pushgit.stagingSectionHeights";

export const UNSTAGED_MIN = 60;
export const UNSTAGED_MAX = 600;
export const UNSTAGED_DEFAULT = 140;
export const STAGED_MIN = 60;
export const STAGED_MAX = 600;
export const STAGED_DEFAULT = 140;

interface PersistedSectionHeights {
  unstaged: number;
  staged: number;
}

function isValidHeight(value: unknown, min: number, max: number): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= min && value <= max;
}

/** Pure parsing/validation, split out from the module-load-time read so it's testable
 *  without fighting this module's own localStorage read at import time. */
export function parseSectionHeightsJson(raw: string | null): PersistedSectionHeights {
  try {
    if (!raw) {
      return { unstaged: UNSTAGED_DEFAULT, staged: STAGED_DEFAULT };
    }
    const parsed: unknown = JSON.parse(raw);
    const candidate = parsed as Partial<PersistedSectionHeights> | null;
    return {
      unstaged: isValidHeight(candidate?.unstaged, UNSTAGED_MIN, UNSTAGED_MAX)
        ? candidate.unstaged
        : UNSTAGED_DEFAULT,
      staged: isValidHeight(candidate?.staged, STAGED_MIN, STAGED_MAX)
        ? candidate.staged
        : STAGED_DEFAULT,
    };
  } catch {
    return { unstaged: UNSTAGED_DEFAULT, staged: STAGED_DEFAULT };
  }
}

export const sectionHeightsState: PersistedSectionHeights = $state(
  parseSectionHeightsJson(localStorage.getItem(STORAGE_KEY)),
);

function persist(): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(sectionHeightsState));
}

export function setUnstagedHeight(value: number): void {
  sectionHeightsState.unstaged = Math.min(UNSTAGED_MAX, Math.max(UNSTAGED_MIN, value));
  persist();
}

export function setStagedHeight(value: number): void {
  sectionHeightsState.staged = Math.min(STAGED_MAX, Math.max(STAGED_MIN, value));
  persist();
}
