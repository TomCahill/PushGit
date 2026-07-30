// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// TypeScript mirror of the backend's commit graph contract
// (`src-tauri/src/graph/model.rs` and `src-tauri/src/graph/lane.rs`). See
// Kept in sync by hand until/unless drift becomes a
// problem.

export type RefKind = "local_branch" | "remote_branch" | "tag";

export interface RefMarker {
  name: string;
  kind: RefKind;
  isHead: boolean;
}

export type RailKind = "pass_through" | "parent_edge" | "merge_edge";

export interface Rail {
  fromLane: number;
  toLane: number;
  colorId: number;
  kind: RailKind;
}

export type RowKind = "commit" | "workdir" | "stash";

export interface CommitRow {
  oid: string;
  shortOid: string;
  row: number;
  summary: string;
  /** The commit message with the summary line stripped. `null` when the message is just a
   *  single line. */
  body: string | null;
  authorName: string;
  authorEmail: string;
  authorTime: number;
  committerTime: number;
  parents: string[];
  isMerge: boolean;
  lane: number;
  colorId: number;
  refs: RefMarker[];
  rails: Rail[];
  kind: RowKind;
  /** Set only for `kind === "stash"` — the stash list index (`stash@{N}`) `applyStash`/
   *  `popStash`/`dropStash` need. */
  stashIndex: number | null;
}

export interface GraphFilter {
  refs: string[];
  /** Free-text search: SHA prefix, or a substring of the commit message, author name, or
   *  any ref pointing at it. Empty means "no filter". */
  search: string;
}

export interface CommitGraphPage {
  rows: CommitRow[];
  hasMore: boolean;
}

// Mirror of `src-tauri/src/diff/model.rs`, only the parts the graph-selection file list
// needs — full hunk rendering is a later phase.

export type FileStatus =
  | "added"
  | "deleted"
  | "modified"
  | "renamed"
  | "copied"
  | "typechange"
  | "conflicted"
  | "untracked"
  | "unreadable";

export type LineOrigin = "addition" | "deletion" | "context";

export interface Line {
  origin: LineOrigin;
  content: string;
  oldLineno: number | null;
  newLineno: number | null;
}

// `Hunk` also round-trips back to the backend as the argument to `stage_hunk`/`unstage_hunk`
// (`src-tauri/src/diff/model.rs`) — the frontend sends back exactly the hunk it was shown.
export interface Hunk {
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: Line[];
}

export interface FileDiff {
  oldPath: string | null;
  newPath: string | null;
  status: FileStatus;
  isBinary: boolean;
  hunks: Hunk[];
  insertions: number;
  deletions: number;
}

/** What a file-list panel (`StagingPanel`, `CommitDiffView`) reports up to the shell
 *  (`+page.svelte`) when a file is selected, so the shell can render `HunkDiff` for it in
 *  the center panel — the `FileDiff` itself plus whichever of `HunkDiff`'s own stage/unstage
 *  action props apply (none, for a read-only selected-commit diff). */
export interface FileDiffSelection {
  file: FileDiff;
  hunkActionLabel?: string;
  onHunkAction?: (hunk: Hunk) => void;
  lineActionLabel?: string;
  onLineAction?: (hunk: Hunk, lineIndices: number[]) => void;
}

// Mirror of `src-tauri/src/diff/model.rs`'s `ConflictSides`.
export interface ConflictSides {
  base: string | null;
  ours: string | null;
  theirs: string | null;
  isBinary: boolean;
  /** The diff between `ours` (old) and `theirs` (new) — the regions needing resolution. */
  hunks: Hunk[];
}

// Mirror of `src-tauri/src/branch/model.rs`.

export interface BranchInfo {
  name: string;
  isHead: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
}

export type MergeOutcome =
  | { kind: "fast_forward" }
  | { kind: "already_up_to_date" }
  | { kind: "merged" }
  | { kind: "conflicts"; conflicts: string[] };

export type RebaseOutcome = { kind: "completed" } | { kind: "conflicts"; conflicts: string[] };

// Mirrors `branch::model::ResetMode`'s `#[serde(rename_all = "snake_case")]`.
export type ResetMode = "soft" | "mixed" | "hard";

export type CherryPickOutcome =
  { kind: "cherry_picked"; oid: string } | { kind: "conflicts"; conflicts: string[] };

// Whether a merge, rebase, or cherry-pick is currently paused mid-conflict.
export type RepoState = "clean" | "merge" | "rebase" | "cherry_pick" | "other";

// Mirror of `src-tauri/src/workflow/model.rs`.
export interface WorkflowConfig {
  main: string;
  develop: string;
  featurePrefix: string;
  releasePrefix: string;
  hotfixPrefix: string;
  supportPrefix: string | null;
  versionTagPrefix: string;
}

export type WorkflowBranchKind = "feature" | "release" | "hotfix";

export type FinishOutcome = { kind: "finished" } | { kind: "conflicts"; conflicts: string[] };

// Mirror of `src-tauri/src/stash/mod.rs`'s `StashEntry`.
export interface StashEntry {
  index: number;
  message: string;
  oid: string;
}

// Mirror of `src-tauri/src/remote/version.rs`'s `GitVersionCheck`.
export interface GitVersionCheck {
  version: string;
  isPatched: boolean;
}

// Mirror of `src-tauri/src/remote/progress.rs`'s `RemoteProgress`.
export interface RemoteProgress {
  phase: string;
  percent: number;
  current: number | null;
  total: number | null;
}

// Mirror of `src-tauri/src/undo/stack.rs`.
export interface UndoRedoStatus {
  canUndo: boolean;
  undoLabel: string | null;
  canRedo: boolean;
  redoLabel: string | null;
}

export interface OperationSummary {
  label: string;
}

// Mirror of `src-tauri/src/interactive_rebase/mod.rs`.
export interface RebaseCommitSummary {
  oid: string;
  shortOid: string;
  summary: string;
}

export type RebaseAction =
  { kind: "pick" } | { kind: "squash" } | { kind: "reword"; message: string } | { kind: "drop" };

export interface RebaseStep {
  oid: string;
  action: RebaseAction;
}

// Mirror of `src-tauri/src/blame/model.rs`.
export interface BlameLine {
  lineNo: number;
  content: string;
  oid: string;
  shortOid: string;
  authorName: string;
  authorTime: number;
  summary: string;
}

export interface FileHistoryEntry {
  oid: string;
  shortOid: string;
  summary: string;
  authorName: string;
  authorTime: number;
}

// Mirror of `src-tauri/src/maintenance/mod.rs`.
export interface RepoHealth {
  looseObjectCount: number;
  looseObjectSizeKib: number;
  packCount: number;
  packedSizeKib: number;
}

// Mirror of `src-tauri/src/config/mod.rs`.
export interface AppConfig {
  maxCommitsRendered: number;
  reduceMotion: boolean;
}

export interface RepoConfig {
  defaultSkipHooks: boolean;
}
