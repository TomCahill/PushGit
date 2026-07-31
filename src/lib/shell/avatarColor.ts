// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Solid colors (unlike the graph's theme-tinted pairs), each checked for contrast against white text.
const PALETTE: readonly string[] = [
  "#6B4FBB", // violet
  "#1F7A5C", // green
  "#B5541D", // orange/brown
  "#1D6FB5", // blue
  "#A02B5C", // magenta
  "#4E7A1D", // olive
  "#B5321D", // red
  "#1D8A8C", // teal
];

/** Deterministic djb2-style string hash. */
function hashString(value: string): number {
  let hash = 5381;
  for (let i = 0; i < value.length; i++) {
    hash = (hash * 33) ^ value.charCodeAt(i);
  }
  return hash >>> 0;
}

/** Stable background color for a given identity seed (email, or name as fallback). */
export function colorForIdentity(seed: string): string {
  return PALETTE[hashString(seed) % PALETTE.length];
}
