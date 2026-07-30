// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Shared builders for component tests — keeps
// `CommitGraph.svelte.test.ts`, `StagingPanel.svelte.test.ts`, and `page.svelte.test.ts`
// from hand-rolling the same boilerplate payload shapes.
import type {
  BranchInfo,
  CommitRow,
  ConflictSides,
  FileDiff,
  Hunk,
  RebaseCommitSummary,
  StashEntry,
} from "./types";

export function makeCommitRow(overrides: Partial<CommitRow> = {}): CommitRow {
  return {
    oid: "0".repeat(40),
    shortOid: "0000000",
    row: 0,
    summary: "A commit",
    body: null,
    authorName: "Ada Lovelace",
    authorEmail: "ada@example.com",
    authorTime: 1700000000,
    committerTime: 1700000000,
    parents: [],
    isMerge: false,
    lane: 0,
    colorId: 0,
    refs: [],
    rails: [],
    kind: "commit",
    stashIndex: null,
    ...overrides,
  };
}

export function makeHunk(overrides: Partial<Hunk> = {}): Hunk {
  return {
    header: "@@ -1,1 +1,1 @@",
    oldStart: 1,
    oldLines: 1,
    newStart: 1,
    newLines: 1,
    lines: [{ origin: "addition", content: "hello\n", oldLineno: null, newLineno: 1 }],
    ...overrides,
  };
}

export function makeFileDiff(overrides: Partial<FileDiff> = {}): FileDiff {
  return {
    oldPath: null,
    newPath: "file.txt",
    status: "modified",
    isBinary: false,
    hunks: [makeHunk()],
    // Matches makeHunk()'s default single addition line.
    insertions: 1,
    deletions: 0,
    ...overrides,
  };
}

export function makeBranchInfo(overrides: Partial<BranchInfo> = {}): BranchInfo {
  return {
    name: "main",
    isHead: false,
    upstream: null,
    ahead: 0,
    behind: 0,
    ...overrides,
  };
}

export function makeRebaseCommitSummary(
  overrides: Partial<RebaseCommitSummary> = {},
): RebaseCommitSummary {
  return {
    oid: "0".repeat(40),
    shortOid: "0000000",
    summary: "A commit",
    ...overrides,
  };
}

export function makeStashEntry(overrides: Partial<StashEntry> = {}): StashEntry {
  return {
    index: 0,
    message: "On main: WIP",
    oid: "0".repeat(40),
    ...overrides,
  };
}

/** A single-hunk ours/theirs disagreement on an otherwise 1-line file, for
 *  `ConflictEditor.svelte.test.ts`. */
export function makeConflictSides(overrides: Partial<ConflictSides> = {}): ConflictSides {
  return {
    base: "base\n",
    ours: "main version\n",
    theirs: "feature version\n",
    isBinary: false,
    hunks: [
      {
        header: "@@ -1,1 +1,1 @@",
        oldStart: 1,
        oldLines: 1,
        newStart: 1,
        newLines: 1,
        lines: [
          { origin: "deletion", content: "main version\n", oldLineno: 1, newLineno: null },
          { origin: "addition", content: "feature version\n", oldLineno: null, newLineno: 1 },
        ],
      },
    ],
    ...overrides,
  };
}
