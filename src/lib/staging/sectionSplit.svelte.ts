// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Stored as a fraction rather than pixels so the split keeps its proportions when the window is resized.
const STORAGE_KEY = "pushgit.stagingSectionSplit";
const LEGACY_HEIGHTS_KEY = "pushgit.stagingSectionHeights";

export const SECTION_MIN_PX = 60;
export const UNSTAGED_FRACTION_DEFAULT = 0.5;

function isValidFraction(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 && value < 1;
}

function isPositiveNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value > 0;
}

function fractionFromLegacyHeights(raw: string | null): number | null {
  if (!raw) return null;
  const parsed = JSON.parse(raw) as { unstaged?: unknown; staged?: unknown } | null;
  const unstaged = parsed?.unstaged;
  const staged = parsed?.staged;
  if (!isPositiveNumber(unstaged) || !isPositiveNumber(staged)) return null;
  return unstaged / (unstaged + staged);
}

export function parseUnstagedFraction(raw: string | null, legacyRaw: string | null): number {
  try {
    if (raw) {
      const parsed = JSON.parse(raw) as { unstagedFraction?: unknown } | null;
      return isValidFraction(parsed?.unstagedFraction)
        ? parsed.unstagedFraction
        : UNSTAGED_FRACTION_DEFAULT;
    }
    return fractionFromLegacyHeights(legacyRaw) ?? UNSTAGED_FRACTION_DEFAULT;
  } catch {
    return UNSTAGED_FRACTION_DEFAULT;
  }
}

export function resizedUnstagedFraction(
  fraction: number,
  deltaPx: number,
  availablePx: number,
): number {
  if (availablePx <= 2 * SECTION_MIN_PX) return fraction;
  const maxPx = availablePx - SECTION_MIN_PX;
  const unstagedPx = Math.min(maxPx, Math.max(SECTION_MIN_PX, fraction * availablePx + deltaPx));
  return unstagedPx / availablePx;
}

export const sectionSplitState: { unstagedFraction: number } = $state({
  unstagedFraction: parseUnstagedFraction(
    localStorage.getItem(STORAGE_KEY),
    localStorage.getItem(LEGACY_HEIGHTS_KEY),
  ),
});

export function setUnstagedFraction(value: number): void {
  sectionSplitState.unstagedFraction = value;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(sectionSplitState));
}
