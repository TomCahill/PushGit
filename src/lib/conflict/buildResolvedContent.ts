// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { Hunk } from "$lib/git/types";

export type HunkDecision =
  { kind: "ours" } | { kind: "theirs" } | { kind: "manual"; content: string };

/**
 * Reconstructs the resolved file from `oursText` plus a per-hunk decision. `hunks` is the
 * ours(old)-vs-theirs(new) diff (`ConflictSides.hunks`) — regions *outside* any hunk are
 * identical between ours/theirs by definition, so they're copied from `oursText` untouched;
 * regions *inside* a hunk are replaced by whichever side (or manual edit) was chosen.
 *
 * An undecided hunk defaults to "theirs" so this can drive a live preview before every hunk
 * has an explicit choice — callers that need to know whether resolution is actually complete
 * should check `decisions.size === hunks.length` themselves before treating the result as final.
 */
export function buildResolvedContent(
  oursText: string,
  hunks: Hunk[],
  decisions: Map<number, HunkDecision>,
): string {
  const oursLines = oursText === "" ? [] : oursText.split(/(?<=\n)/);
  const sorted = hunks
    .map((hunk, index) => ({ hunk, index }))
    .sort((a, b) => a.hunk.oldStart - b.hunk.oldStart);

  let cursor = 0;
  let result = "";

  for (const { hunk, index } of sorted) {
    const hunkStart = Math.max(hunk.oldStart - 1, 0);
    result += oursLines.slice(cursor, hunkStart).join("");

    const decision = decisions.get(index) ?? { kind: "theirs" as const };
    if (decision.kind === "manual") {
      result += decision.content;
    } else {
      const wanted = decision.kind === "ours" ? ["context", "deletion"] : ["context", "addition"];
      result += hunk.lines
        .filter((line) => wanted.includes(line.origin))
        .map((line) => line.content)
        .join("");
    }

    cursor = hunkStart + hunk.oldLines;
  }

  result += oursLines.slice(cursor).join("");
  return result;
}
