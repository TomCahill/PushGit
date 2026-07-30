// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Data contract for GitFlow configuration and start/finish outcomes.

use serde::{Deserialize, Serialize};

/// Mirrors the `git-flow` (AVH edition) `.git/config` schema exactly — `detect_workflow`
/// reads these same keys and `init_workflow` writes them, so a repo configured via the real
/// `git flow init` CLI works in PushGit with no migration and vice versa. `support_prefix`
/// stays optional and unused beyond round-tripping: this pass builds no start/finish for
/// git-flow's support-branch type, but a repo that already has one configured (via the CLI's
/// own interactive prompt) shouldn't have that setting silently dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowConfig {
    pub main: String,
    pub develop: String,
    pub feature_prefix: String,
    pub release_prefix: String,
    pub hotfix_prefix: String,
    pub support_prefix: Option<String>,
    pub version_tag_prefix: String,
}

/// Pre-fill values for PushGit's own "Set up GitFlow" form — never assumed when *reading* an
/// existing config, only used to seed a fresh `init_workflow` call. Matches `git-flow`'s own
/// defaults exactly except `main` (the CLI defaults to `master`; PushGit pre-fills `main`
/// since that's git's modern default branch name).
impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            main: "main".to_string(),
            develop: "develop".to_string(),
            feature_prefix: "feature/".to_string(),
            release_prefix: "release/".to_string(),
            hotfix_prefix: "hotfix/".to_string(),
            support_prefix: None,
            version_tag_prefix: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowBranchKind {
    Feature,
    Release,
    Hotfix,
}

/// Shape mirrors `MergeOutcome`'s own `Conflicts` payload convention — `finish_branch` can
/// pause on either of its two possible merge steps (into `main`, then into `develop`) and
/// this collapses both into the same "here are the conflicted paths" shape the existing
/// conflict-resolution UI already understands.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "conflicts")]
pub enum FinishOutcome {
    Finished,
    Conflicts(Vec<String>),
}
