// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { Line } from "$lib/git/types";

export interface PairedLine {
  line: Line;
  /** Index into the source hunk's `lines` array — lets a renderer look up per-line data
   *  (e.g. highlighted HTML) keyed by position without re-deriving it. */
  index: number;
}

export interface PairedRow {
  left: PairedLine | null;
  right: PairedLine | null;
}

/**
 * Pairs a hunk's flat `lines` (context/deletion/addition, in diff order) into side-by-side
 * rows: a context line renders identically in both columns, and each contiguous
 * deletion-then-addition "change block" is zipped pairwise, padding the shorter side with a
 * blank cell — the same alignment GitHub/GitLab's split diff view uses.
 */
export function pairHunkLines(lines: Line[]): PairedRow[] {
  const rows: PairedRow[] = [];
  let i = 0;

  while (i < lines.length) {
    if (lines[i].origin === "context") {
      const contextLine = { line: lines[i], index: i };
      rows.push({ left: contextLine, right: contextLine });
      i++;
      continue;
    }

    const deletions: PairedLine[] = [];
    while (i < lines.length && lines[i].origin === "deletion") {
      deletions.push({ line: lines[i], index: i });
      i++;
    }
    const additions: PairedLine[] = [];
    while (i < lines.length && lines[i].origin === "addition") {
      additions.push({ line: lines[i], index: i });
      i++;
    }

    const count = Math.max(deletions.length, additions.length);
    for (let j = 0; j < count; j++) {
      rows.push({ left: deletions[j] ?? null, right: additions[j] ?? null });
    }
  }

  return rows;
}
