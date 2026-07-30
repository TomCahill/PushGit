// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The backend→frontend commit graph data contract —
//! the frontend is a "dumb renderer" that draws exactly this data and never reconstructs
//! topology, lane assignment, or color identity itself.

use serde::{Deserialize, Serialize};

use super::lane::Rail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// What a `CommitRow` represents: the "WIP row" and this app's
/// stash-on-the-graph addition — both are spliced into the same paginated row list a real
/// commit occupies, distinguished only by this tag, so the frontend's existing lane/rail
/// rendering needs no special-casing to draw the connecting lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Commit,
    /// A live, synthetic row for the working directory's uncommitted state — not a real
    /// commit object. `oid` is [`WORKDIR_OID`], the all-zero sentinel git tooling
    /// conventionally uses for "no object" (e.g. `git diff --no-index`).
    Workdir,
    /// A real stash commit, rendered like an ordinary commit but anchored only to
    /// `parents[0]` (the commit it was taken from) — its internal index/untracked-file
    /// parents are neither walked nor displayed.
    Stash,
}

/// Sentinel `oid` for a [`RowKind::Workdir`] row — not a real object id.
pub const WORKDIR_OID: &str = "0000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefMarker {
    pub name: String,
    pub kind: RefKind,
    pub is_head: bool,
}

/// One commit = one row. The full contract with the frontend renderer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRow {
    pub oid: String,
    pub short_oid: String,
    /// Absolute position in the topo-sorted log; stable virtualization key.
    pub row: u32,
    pub summary: String,
    /// The commit message with the summary line stripped, per `git2::Commit::body`. `None`
    /// when the message is just a single line.
    pub body: Option<String>,
    pub author_name: String,
    pub author_email: String,
    /// Unix seconds.
    pub author_time: i64,
    /// Unix seconds; shown when it differs from `author_time` (rebases/amends).
    pub committer_time: i64,
    /// `parents[0]` is first-parent.
    pub parents: Vec<String>,
    pub is_merge: bool,

    pub lane: u16,
    /// Stable palette index.
    pub color_id: u16,

    /// Branches/tags/HEAD resolving directly to this commit.
    pub refs: Vec<RefMarker>,
    /// Every active lane's segment through this row, not just the ones touching this
    /// commit — this is what lets the frontend draw a fully-continuous graph.
    pub rails: Vec<Rail>,

    pub kind: RowKind,
    /// Set only for `kind == Stash` — the stash list index (`stash@{N}`) `apply_stash`/
    /// `pop_stash`/`drop_stash` need.
    pub stash_index: Option<usize>,
}

/// Branch/tag ref-scope and search criteria for a graph session, for the
/// "Graph search/filter" row.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphFilter {
    /// If non-empty, only these refs (and their ancestry) are walked. Empty means "all
    /// branches" (`git2::Repository::branches(None)`).
    pub refs: Vec<String>,
    /// Free-text search, matched case-insensitively against a commit's SHA (prefix),
    /// summary, author name, and any ref pointing at it. Empty means "no filter" — the
    /// common case, so it costs nothing beyond a length check when unused.
    #[serde(default)]
    pub search: String,
}

/// One page of rows, returned by the paginated `graph_page` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitGraphPage {
    pub rows: Vec<CommitRow>,
    pub has_more: bool,
}
