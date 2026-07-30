// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Data contract for branch listing, merge, and rebase outcomes.

use serde::{Deserialize, Serialize};

/// Mirrors `git2::ResetType`'s three modes exactly — `reset_to` translates one straight
/// through to it. Kept as its own type (rather than exposing `git2::ResetType` directly)
/// so this module doesn't leak a `git2` type across the Tauri IPC boundary.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "conflicts")]
pub enum MergeOutcome {
    FastForward,
    AlreadyUpToDate,
    Merged,
    Conflicts(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "conflicts")]
pub enum RebaseOutcome {
    Completed,
    Conflicts(Vec<String>),
}

/// `content = "oid"` for `CherryPicked`, matching `MergeOutcome`'s `conflicts` naming
/// convention for its own payload field.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CherryPickOutcome {
    CherryPicked { oid: String },
    Conflicts { conflicts: Vec<String> },
}

/// Simplified view of `git2::RepositoryState` — collapses the rebase sub-variants
/// (`Rebase`/`RebaseInteractive`/`RebaseMerge`) into one, since PushGit only ever produces
/// the plain `RebaseMerge` state itself and the frontend just needs to know "am I mid-merge,
/// mid-rebase, or mid-cherry-pick" to offer the right recovery actions (abort / continue).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoState {
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Other,
}

impl From<git2::RepositoryState> for RepoState {
    fn from(state: git2::RepositoryState) -> Self {
        match state {
            git2::RepositoryState::Clean => RepoState::Clean,
            git2::RepositoryState::Merge => RepoState::Merge,
            git2::RepositoryState::Rebase
            | git2::RepositoryState::RebaseInteractive
            | git2::RepositoryState::RebaseMerge => RepoState::Rebase,
            git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
                RepoState::CherryPick
            }
            _ => RepoState::Other,
        }
    }
}
