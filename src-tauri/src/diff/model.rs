// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The diff/patch data contract handed to the frontend. Syntax highlighting and
//! side-by-side/inline layout are frontend rendering concerns — this
//! module only computes the underlying hunks/lines.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    Typechange,
    Conflicted,
    /// Present in the working directory but not yet added to the index at all (distinct
    /// from `Added`, which means "staged as a new file").
    Untracked,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineOrigin {
    Addition,
    Deletion,
    Context,
}

/// `Deserialize` because `Hunk`/`Line` round-trip back from the frontend as the argument
/// to `stage_hunk`/`unstage_hunk` — the frontend sends back exactly the hunk it was shown.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub origin: LineOrigin,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: FileStatus,
    /// True for binary files; `hunks` is always empty in that case rather than attempting
    /// to render binary content as text.
    pub is_binary: bool,
    pub hunks: Vec<Hunk>,
    /// Addition/deletion line counts across `hunks`, for a file-list "+N -M" summary
    /// without the frontend re-walking every hunk itself. Always 0/0 for a binary file.
    pub insertions: u32,
    pub deletions: u32,
}

/// The three sides of an unresolved merge conflict at one path, plus the diff between
/// `ours` and `theirs` — the regions that actually need a resolution choice, since a line
/// both sides agree on is unambiguous regardless of what the base said. A side is `None`
/// when it doesn't exist for this conflict (e.g. an add/add conflict has no `base`; a
/// modify/delete conflict has no `ours` or no `theirs`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSides {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    /// True when `ours` or `theirs` isn't valid UTF-8 text; `hunks` is always empty in that
    /// case, matching `FileDiff::is_binary`.
    pub is_binary: bool,
    pub hunks: Vec<Hunk>,
}
