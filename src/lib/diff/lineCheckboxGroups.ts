// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { Line } from "$lib/git/types";

/**
 * Groups a hunk's lines for line-level checkbox rendering, mirroring the backend's
 * `expand_replacement_groups` (`src-tauri/src/stage/mod.rs`): a maximal run of consecutive
 * non-context lines containing *both* a deletion and an addition — how git represents an
 * edited line (delete the old content, add the new) — is staged/unstaged as one atomic
 * unit, so it gets a single checkbox rather than one per line. A pure-addition or
 * pure-deletion run (a genuine standalone insertion or removal) stays independently
 * selectable, one checkbox per line.
 *
 * Returns an array index-aligned with `lines`: `null` for a context line or a
 * non-representative member of a replacement group (no checkbox rendered there), otherwise
 * the full list of line indices that checkbox acts on — `[index]` for a standalone line,
 * or every index in the group when rendered on the group's first line.
 */
export function lineCheckboxGroups(lines: Line[]): (number[] | null)[] {
  const groups: (number[] | null)[] = new Array(lines.length).fill(null);
  let i = 0;

  while (i < lines.length) {
    if (lines[i].origin === "context") {
      i++;
      continue;
    }

    const start = i;
    while (i < lines.length && lines[i].origin !== "context") {
      i++;
    }
    const indices = Array.from({ length: i - start }, (_, k) => start + k);
    const hasAddition = indices.some((idx) => lines[idx].origin === "addition");
    const hasDeletion = indices.some((idx) => lines[idx].origin === "deletion");

    if (hasAddition && hasDeletion) {
      groups[start] = indices;
    } else {
      for (const idx of indices) {
        groups[idx] = [idx];
      }
    }
  }

  return groups;
}
