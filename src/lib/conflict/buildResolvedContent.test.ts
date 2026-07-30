// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import type { Hunk, Line } from "$lib/git/types";
import { buildResolvedContent } from "./buildResolvedContent";

function line(origin: Line["origin"], content: string): Line {
  return { origin, content, oldLineno: null, newLineno: null };
}

// ours = "line1\nline2\nline3\nline4\nline5\n", theirs = "line1\nline2\nLINE3\nline4\nline5\n"
const singleHunk: Hunk = {
  header: "@@ -2,3 +2,3 @@",
  oldStart: 2,
  oldLines: 3,
  newStart: 2,
  newLines: 3,
  lines: [
    line("context", "line2\n"),
    line("deletion", "line3\n"),
    line("addition", "LINE3\n"),
    line("context", "line4\n"),
  ],
};
const oursText = "line1\nline2\nline3\nline4\nline5\n";
const theirsText = "line1\nline2\nLINE3\nline4\nline5\n";

describe("buildResolvedContent", () => {
  it("returns oursText unchanged when there are no hunks", () => {
    expect(buildResolvedContent(oursText, [], new Map())).toBe(oursText);
  });

  it("reconstructs the ours text when 'ours' is chosen", () => {
    const result = buildResolvedContent(oursText, [singleHunk], new Map([[0, { kind: "ours" }]]));
    expect(result).toBe(oursText);
  });

  it("reconstructs the theirs text when 'theirs' is chosen", () => {
    const result = buildResolvedContent(oursText, [singleHunk], new Map([[0, { kind: "theirs" }]]));
    expect(result).toBe(theirsText);
  });

  it("splices in manual content for a manually-edited hunk", () => {
    // Manual content replaces the hunk's whole range (including its context padding, since
    // that's what the editor pre-fills the textarea with), not just the conflicting lines.
    const result = buildResolvedContent(
      oursText,
      [singleHunk],
      new Map([[0, { kind: "manual", content: "line2\nline3 and LINE3\nline4\n" }]]),
    );
    expect(result).toBe("line1\nline2\nline3 and LINE3\nline4\nline5\n");
  });

  it("defaults an undecided hunk to 'theirs' for a live preview", () => {
    const result = buildResolvedContent(oursText, [singleHunk], new Map());
    expect(result).toBe(theirsText);
  });

  it("resolves multiple hunks independently, out of order", () => {
    // ours = "a\nb\nc\nd\ne\n", theirs = "A\nb\nc\nd\nE\n" (first and last lines differ)
    const first: Hunk = {
      header: "@@ -1,1 +1,1 @@",
      oldStart: 1,
      oldLines: 1,
      newStart: 1,
      newLines: 1,
      lines: [line("deletion", "a\n"), line("addition", "A\n")],
    };
    const second: Hunk = {
      header: "@@ -5,1 +5,1 @@",
      oldStart: 5,
      oldLines: 1,
      newStart: 5,
      newLines: 1,
      lines: [line("deletion", "e\n"), line("addition", "E\n")],
    };
    const ours = "a\nb\nc\nd\ne\n";

    // Passed as [second, first] (array indices 0=second, 1=first) to confirm reconstruction
    // follows sorted oldStart order, not input-array order — decisions are still keyed by
    // each hunk's index in the *input* array, matching how a component would track them
    // against a single `{#each hunks as hunk, index}` pass over the array it was given.
    const decisions = new Map<number, import("./buildResolvedContent").HunkDecision>([
      [1, { kind: "theirs" }], // first (a -> A)
      [0, { kind: "ours" }], // second (stays e)
    ]);
    const result = buildResolvedContent(ours, [second, first], decisions);

    expect(result).toBe("A\nb\nc\nd\ne\n");
  });

  it("handles a hunk covering content with no trailing newline", () => {
    const ours = "only line";
    const hunk: Hunk = {
      header: "@@ -1,1 +1,1 @@",
      oldStart: 1,
      oldLines: 1,
      newStart: 1,
      newLines: 1,
      lines: [line("deletion", "only line"), line("addition", "changed line")],
    };

    expect(buildResolvedContent(ours, [hunk], new Map([[0, { kind: "ours" }]]))).toBe(ours);
    expect(buildResolvedContent(ours, [hunk], new Map([[0, { kind: "theirs" }]]))).toBe(
      "changed line",
    );
  });
});
