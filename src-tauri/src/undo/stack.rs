// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The undo/redo stack itself: two `Vec`s of captured `Snapshot`s per open repo, plus the
//! git-plumbing bookkeeping that keeps each entry's objects alive until it's restored to or
//! evicted. Keyed by repo path, like `remote::CancellationRegistry` — but backed by a plain
//! `std::sync::Mutex`, not `tokio::sync::Mutex` (the usual rule for this codebase):
//! every operation here is synchronous git2 work with no `.await` in its own critical
//! section, and `git2::Repository` is `Send` but not `Sync`, so a `&Repository` held across
//! an actual `.await` point would make the enclosing Tauri command's future non-`Send` — the
//! thing that rule exists to avoid in the *other* direction.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use git2::{Commit, Repository};
use serde::Serialize;

use super::{capture, keep_alive_oids, restore, Snapshot};
use crate::error::{PushGitError, PushGitResult};

/// Oldest entries are evicted past this depth, per repo, on both the undo and redo side —
/// bounds the number of keep-alive refs/manifest commits a long session accumulates.
const MAX_DEPTH: usize = 50;

struct OperationEntry {
    slot: u64,
    label: String,
    snapshot: Snapshot,
}

#[derive(Default)]
struct RepoUndoStack {
    undo: Vec<OperationEntry>,
    redo: Vec<OperationEntry>,
    next_slot: u64,
    /// A snapshot captured before a *multi-step* operation (interactive rebase: start, then
    /// zero or more continues) started, held here — not yet in `undo` — until the whole
    /// sequence resolves. See `UndoLog::hold_pending`/`resolve_pending`.
    pending: Option<OperationEntry>,
}

/// Per-repo undo/redo history, keyed by repo path exactly like `CancellationRegistry` —
/// managed as Tauri `State<AppState>`.
#[derive(Default)]
pub struct UndoLog {
    stacks: Mutex<HashMap<String, RepoUndoStack>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoRedoStatus {
    pub can_undo: bool,
    pub undo_label: Option<String>,
    pub can_redo: bool,
    pub redo_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSummary {
    pub label: String,
}

impl UndoLog {
    /// Captures the repo's current state. Call before running a wrapped operation, then
    /// pass the result to `push` only if the operation should actually be undoable — merge
    /// and rebase, for example, skip this when they end in a conflict, since the in-progress
    /// conflict state already has its own abort/continue recovery path.
    pub fn capture(&self, repo: &Repository) -> PushGitResult<Snapshot> {
        capture(repo)
    }

    /// Records `label` as an undoable entry that restores back to `snapshot` (the state
    /// captured just before the operation ran). Clears the redo stack — a fresh action
    /// invalidates whatever was available to redo, matching ordinary editor undo semantics —
    /// and evicts the oldest undo entry once the stack exceeds `MAX_DEPTH`.
    pub fn push(
        &self,
        repo: &Repository,
        repo_path: &str,
        label: impl Into<String>,
        snapshot: Snapshot,
    ) -> PushGitResult<()> {
        let label = label.into();
        let mut stacks = self.lock();
        let stack = stacks.entry(repo_path.to_string()).or_default();

        for entry in stack.redo.drain(..) {
            release_slot(repo, entry.slot)?;
        }

        let slot = stack.next_slot;
        stack.next_slot += 1;
        anchor_slot(repo, slot, &snapshot, &label)?;
        stack.undo.push(OperationEntry {
            slot,
            label,
            snapshot,
        });

        if stack.undo.len() > MAX_DEPTH {
            let evicted = stack.undo.remove(0);
            release_slot(repo, evicted.slot)?;
        }

        Ok(())
    }

    /// Undoes the most recent entry: restores its pre-operation snapshot, and pushes the
    /// state being left behind onto the redo stack under the same label, so "Redo" reads as
    /// re-applying the operation just undone.
    pub fn undo(&self, repo: &mut Repository, repo_path: &str) -> PushGitResult<OperationSummary> {
        let mut stacks = self.lock();
        let stack = stacks.entry(repo_path.to_string()).or_default();

        let Some(entry) = stack.undo.pop() else {
            return Err(PushGitError::Invalid("nothing to undo".to_string()));
        };

        let current = capture(repo)?;
        let redo_slot = stack.next_slot;
        stack.next_slot += 1;
        anchor_slot(repo, redo_slot, &current, &entry.label)?;
        stack.redo.push(OperationEntry {
            slot: redo_slot,
            label: entry.label.clone(),
            snapshot: current,
        });

        restore(repo, &entry.snapshot)?;
        release_slot(repo, entry.slot)?;

        Ok(OperationSummary { label: entry.label })
    }

    /// Symmetric counterpart to `undo` — re-applies the most recently undone entry.
    pub fn redo(&self, repo: &mut Repository, repo_path: &str) -> PushGitResult<OperationSummary> {
        let mut stacks = self.lock();
        let stack = stacks.entry(repo_path.to_string()).or_default();

        let Some(entry) = stack.redo.pop() else {
            return Err(PushGitError::Invalid("nothing to redo".to_string()));
        };

        let current = capture(repo)?;
        let undo_slot = stack.next_slot;
        stack.next_slot += 1;
        anchor_slot(repo, undo_slot, &current, &entry.label)?;
        stack.undo.push(OperationEntry {
            slot: undo_slot,
            label: entry.label.clone(),
            snapshot: current,
        });

        restore(repo, &entry.snapshot)?;
        release_slot(repo, entry.slot)?;

        Ok(OperationSummary { label: entry.label })
    }

    /// Whether undo/redo are currently available for `repo_path`, and what each would do —
    /// drives the frontend's Undo/Redo button labels and disabled state.
    pub fn status(&self, repo_path: &str) -> UndoRedoStatus {
        let stacks = self.lock();
        match stacks.get(repo_path) {
            Some(stack) => UndoRedoStatus {
                can_undo: !stack.undo.is_empty(),
                undo_label: stack.undo.last().map(|e| e.label.clone()),
                can_redo: !stack.redo.is_empty(),
                redo_label: stack.redo.last().map(|e| e.label.clone()),
            },
            None => UndoRedoStatus {
                can_undo: false,
                undo_label: None,
                can_redo: false,
                redo_label: None,
            },
        }
    }

    /// Holds `snapshot` as the pending pre-op state for a multi-step operation (interactive
    /// rebase) that isn't done yet — not yet a real undo entry, since `start`/`continue`
    /// calls are separate command invocations with no other way to carry a snapshot forward
    /// between them. Replaces (releasing) any previous pending snapshot for the same repo,
    /// which can only happen if a prior multi-step operation was abandoned without ever
    /// resolving — see the module doc's known-limitation note in `interactive_rebase`.
    pub fn hold_pending(
        &self,
        repo: &Repository,
        repo_path: &str,
        label: impl Into<String>,
        snapshot: Snapshot,
    ) -> PushGitResult<()> {
        let label = label.into();
        let mut stacks = self.lock();
        let stack = stacks.entry(repo_path.to_string()).or_default();

        if let Some(old) = stack.pending.take() {
            release_slot(repo, old.slot)?;
        }

        let slot = stack.next_slot;
        stack.next_slot += 1;
        anchor_slot(repo, slot, &snapshot, &label)?;
        stack.pending = Some(OperationEntry {
            slot,
            label,
            snapshot,
        });
        Ok(())
    }

    /// Finalizes the pending snapshot held by `hold_pending`: pushes it as a real undo entry
    /// if `commit` is true (the whole multi-step sequence completed), or just releases it
    /// otherwise (aborted). A no-op if nothing is pending.
    pub fn resolve_pending(
        &self,
        repo: &Repository,
        repo_path: &str,
        commit: bool,
    ) -> PushGitResult<()> {
        let mut stacks = self.lock();
        let stack = stacks.entry(repo_path.to_string()).or_default();

        let Some(entry) = stack.pending.take() else {
            return Ok(());
        };

        if commit {
            for old in stack.redo.drain(..) {
                release_slot(repo, old.slot)?;
            }
            stack.undo.push(entry);
            if stack.undo.len() > MAX_DEPTH {
                let evicted = stack.undo.remove(0);
                release_slot(repo, evicted.slot)?;
            }
        } else {
            release_slot(repo, entry.slot)?;
        }
        Ok(())
    }

    /// A poisoned lock (a prior panic while holding it) shouldn't take down the whole undo
    /// stack for every other repo the app happens to have open — recover the possibly
    /// inconsistent-for-one-repo map rather than propagating the poison as a panic here too.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RepoUndoStack>> {
        self.stacks.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Anchors every oid `snapshot` needs to survive `git gc` (the module doc's "Reachability"
/// note) behind one ref, `refs/pushgit/undo/<slot>`, pointing at a throwaway "manifest"
/// commit whose tree is the snapshot's workdir tree and whose parents are every commit the
/// snapshot references — the index tree (wrapped in its own parentless commit, since a tree
/// can't be a commit parent directly) plus every branch tip, tag target, and stash commit.
fn anchor_slot(
    repo: &Repository,
    slot: u64,
    snapshot: &Snapshot,
    label: &str,
) -> PushGitResult<()> {
    let signature = repo.signature()?;

    let index_tree = repo.find_tree(snapshot.index_tree)?;
    let index_marker = repo.commit(
        None,
        &signature,
        &signature,
        "pushgit undo: index",
        &index_tree,
        &[],
    )?;

    let mut parent_oids: HashSet<git2::Oid> = keep_alive_oids(snapshot).into_iter().collect();
    parent_oids.insert(index_marker);

    let parents: Vec<Commit> = parent_oids
        .into_iter()
        .map(|oid| repo.find_commit(oid))
        .collect::<Result<_, _>>()?;
    let parent_refs: Vec<&Commit> = parents.iter().collect();

    let workdir_tree = repo.find_tree(snapshot.workdir_tree)?;
    let manifest = repo.commit(
        None,
        &signature,
        &signature,
        label,
        &workdir_tree,
        &parent_refs,
    )?;

    repo.reference(
        &format!("refs/pushgit/undo/{slot}"),
        manifest,
        true,
        "pushgit undo entry",
    )?;
    Ok(())
}

fn release_slot(repo: &Repository, slot: u64) -> PushGitResult<()> {
    if let Ok(mut reference) = repo.find_reference(&format!("refs/pushgit/undo/{slot}")) {
        reference.delete()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{create_branch, delete_branch};
    use crate::test_support::repo_init;

    #[test]
    fn undo_then_redo_round_trips_a_branch_delete() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        let log = UndoLog::default();

        let snapshot = log.capture(&repo).unwrap();
        delete_branch(&repo, "feature").unwrap();
        log.push(&repo, "repo", "Delete branch 'feature'", snapshot)
            .unwrap();

        let status = log.status("repo");
        assert!(status.can_undo);
        assert_eq!(
            status.undo_label.as_deref(),
            Some("Delete branch 'feature'")
        );
        assert!(!status.can_redo);

        let summary = log.undo(&mut repo, "repo").unwrap();
        assert_eq!(summary.label, "Delete branch 'feature'");
        assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());

        let status = log.status("repo");
        assert!(!status.can_undo);
        assert!(status.can_redo);

        log.redo(&mut repo, "repo").unwrap();
        assert!(repo
            .find_branch("feature", git2::BranchType::Local)
            .is_err());
    }

    #[test]
    fn undo_on_an_empty_stack_errors() {
        let (_dir, mut repo) = repo_init();
        let log = UndoLog::default();

        assert!(log.undo(&mut repo, "repo").is_err());
    }

    #[test]
    fn a_new_push_clears_the_redo_stack() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "a", None).unwrap();
        create_branch(&repo, "b", None).unwrap();
        let log = UndoLog::default();

        let snap_a = log.capture(&repo).unwrap();
        delete_branch(&repo, "a").unwrap();
        log.push(&repo, "repo", "Delete branch 'a'", snap_a)
            .unwrap();
        log.undo(&mut repo, "repo").unwrap();
        assert!(log.status("repo").can_redo);

        let snap_b = log.capture(&repo).unwrap();
        delete_branch(&repo, "b").unwrap();
        log.push(&repo, "repo", "Delete branch 'b'", snap_b)
            .unwrap();

        assert!(!log.status("repo").can_redo);
    }

    #[test]
    fn undo_stack_evicts_the_oldest_entry_past_max_depth() {
        let (_dir, repo) = repo_init();
        let log = UndoLog::default();

        for i in 0..(MAX_DEPTH + 5) {
            let snapshot = log.capture(&repo).unwrap();
            log.push(&repo, "repo", format!("op {i}"), snapshot)
                .unwrap();
        }

        let stacks = log.lock();
        let stack = stacks.get("repo").unwrap();
        assert_eq!(stack.undo.len(), MAX_DEPTH);
        assert_eq!(stack.undo.first().unwrap().label, "op 5");
    }

    #[test]
    fn resolving_a_pending_snapshot_as_committed_pushes_a_real_undo_entry() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        let log = UndoLog::default();

        let snapshot = log.capture(&repo).unwrap();
        delete_branch(&repo, "feature").unwrap();
        log.hold_pending(&repo, "repo", "Interactive rebase", snapshot)
            .unwrap();
        assert!(!log.status("repo").can_undo, "not a real entry yet");

        log.resolve_pending(&repo, "repo", true).unwrap();

        assert!(log.status("repo").can_undo);
        log.undo(&mut repo, "repo").unwrap();
        assert!(repo.find_branch("feature", git2::BranchType::Local).is_ok());
    }

    #[test]
    fn resolving_a_pending_snapshot_as_not_committed_discards_it() {
        let (_dir, repo) = repo_init();
        let log = UndoLog::default();

        let snapshot = log.capture(&repo).unwrap();
        log.hold_pending(&repo, "repo", "Interactive rebase", snapshot)
            .unwrap();

        log.resolve_pending(&repo, "repo", false).unwrap();

        assert!(!log.status("repo").can_undo);
    }

    #[test]
    fn resolve_pending_is_a_no_op_when_nothing_is_pending() {
        let (_dir, repo) = repo_init();
        let log = UndoLog::default();

        log.resolve_pending(&repo, "repo", true).unwrap();

        assert!(!log.status("repo").can_undo);
    }
}
