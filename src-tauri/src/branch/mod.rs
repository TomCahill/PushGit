// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Branch create/checkout/rename/delete/reset, plus repository-state tracking (clean vs.
//! mid-merge/rebase/cherry-pick). Merge, rebase, cherry-pick, conflict resolution, and tag
//! management each live in their own submodule below — split out of what used to be one
//! file once each had grown into its own self-contained concern with no shared state beyond
//! this module's `collect_conflict_paths`/`invalid` helpers and the git2 `Repository` every
//! function here takes by reference.
//!
//! Non-interactive rebase is implemented in `rebase` via git2's own `Rebase` API (the
//! cherry-pick-based `rebase-merge` machinery libgit2 supports natively) — only
//! *interactive* rebase (reorder/squash/reword) is a hard
//! gap requiring the real `git` binary; see `interactive_rebase/`.

mod cherry_pick;
mod conflict;
mod merge;
mod model;
mod rebase;
mod tag;

pub use cherry_pick::{abort_cherry_pick, cherry_pick, is_multi_cherry_pick_in_progress};
pub use conflict::{
    conflict_sides, list_conflicts, resolve_conflict, resolve_conflict_as_deleted,
    write_resolved_conflict,
};
pub use merge::{abort_merge, merge_branch, merge_branch_no_ff};
pub use model::{BranchInfo, CherryPickOutcome, MergeOutcome, RebaseOutcome, RepoState, ResetMode};
pub use rebase::{abort_rebase, continue_rebase, rebase_branch};
pub use tag::{create_tag, delete_tag, list_tags, move_tag, rename_tag};

use git2::{BranchType, Index, Repository, ResetType};

use crate::error::{PushGitError, PushGitResult};

/// Whether the repository is clean or has a merge/rebase paused mid-flight — lets the
/// frontend offer the right recovery action (abort vs. continue) without guessing from
/// which button the user last clicked, which wouldn't survive reopening a repo that was
/// already mid-conflict from outside PushGit.
pub fn repo_state(repo: &Repository) -> RepoState {
    repo.state().into()
}

pub fn list_branches(repo: &Repository) -> PushGitResult<Vec<BranchInfo>> {
    let head_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(str::to_string));
    let mut result = Vec::new();

    for branch in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch?;
        let Some(name) = branch.name()?.map(str::to_string) else {
            continue;
        };

        let is_head = head_name.as_deref() == Some(name.as_str());

        let (upstream_name, ahead, behind) = match branch.upstream() {
            Ok(upstream) => {
                let upstream_name = upstream.name()?.map(str::to_string);
                let counts = match (branch.get().target(), upstream.get().target()) {
                    (Some(local), Some(remote)) => repo.graph_ahead_behind(local, remote)?,
                    _ => (0, 0),
                };
                (upstream_name, counts.0, counts.1)
            }
            Err(_) => (None, 0, 0),
        };

        result.push(BranchInfo {
            name,
            is_head,
            upstream: upstream_name,
            ahead,
            behind,
        });
    }

    Ok(result)
}

/// Creates a local branch at `at` (any commit-ish, e.g. a full/short oid or another ref
/// name), or at the current HEAD if `at` is `None`.
pub fn create_branch(repo: &Repository, name: &str, at: Option<&str>) -> PushGitResult<()> {
    let target = match at {
        Some(rev) => repo.revparse_single(rev)?.peel_to_commit()?,
        None => repo.head()?.peel_to_commit()?,
    };
    repo.branch(name, &target, false)?;
    Ok(())
}

pub fn checkout_branch(repo: &Repository, name: &str) -> PushGitResult<()> {
    let refname = format!("refs/heads/{name}");
    let object = repo.revparse_single(&refname)?;
    repo.checkout_tree(&object, None)?;
    repo.set_head(&refname)?;
    Ok(())
}

/// Checks out an arbitrary commit-ish directly, leaving HEAD detached — for right-clicking a
/// commit dot in the graph, as opposed to `checkout_branch`'s
/// branch-only checkout.
pub fn checkout_commit(repo: &Repository, oid: &str) -> PushGitResult<()> {
    let object = repo.revparse_single(oid)?;
    let commit = object.peel_to_commit()?;
    repo.checkout_tree(&object, None)?;
    repo.set_head_detached(commit.id())?;
    Ok(())
}

/// Checks out `remote_branch_name` (e.g. `"origin/feature"`, the shorthand `Branch::name()`
/// returns for a remote-tracking branch), creating a local branch of the same name tracking it
/// first if one doesn't already exist yet — mirrors plain git's DWIM checkout. Reuses an
/// existing local branch of the same name as-is rather than re-pointing it, matching git's own
/// refusal to silently repoint a branch that's already there.
pub fn checkout_remote_branch(repo: &Repository, remote_branch_name: &str) -> PushGitResult<()> {
    let local_name = remote_branch_name
        .split_once('/')
        .map(|(_, rest)| rest)
        .ok_or_else(|| {
            invalid(format!(
                "'{remote_branch_name}' is not a remote branch name"
            ))
        })?;

    if repo.find_branch(local_name, BranchType::Local).is_err() {
        let remote_branch = repo.find_branch(remote_branch_name, BranchType::Remote)?;
        let target = remote_branch.get().peel_to_commit()?;
        let mut new_branch = repo.branch(local_name, &target, false)?;
        new_branch.set_upstream(Some(remote_branch_name))?;
    }

    checkout_branch(repo, local_name)
}

pub fn rename_branch(repo: &Repository, old_name: &str, new_name: &str) -> PushGitResult<()> {
    let mut branch = repo.find_branch(old_name, BranchType::Local)?;
    branch.rename(new_name, false)?;
    Ok(())
}

/// Deletes a local branch. Deleting its remote-tracking counterpart too (the
/// companion-checkbox UX) is a separate call — that's a push/delete
/// operation belonging to `remote/`, not this function.
pub fn delete_branch(repo: &Repository, name: &str) -> PushGitResult<()> {
    let mut branch = repo.find_branch(name, BranchType::Local)?;
    branch.delete()?;
    Ok(())
}

/// Resets the current branch to `target` — `git reset --soft/--mixed/--hard <target>`'s
/// direct equivalent, via the drag-to-reset gesture (the current
/// branch's own badge, dragged onto a commit row, with a modifier held to distinguish it
/// from the existing drag-to-rebase-onto-a-commit gesture). Reset only ever moves *HEAD*'s
/// own branch — there's no soft/mixed/hard concept for repointing a branch you haven't
/// checked out (that's just moving a ref, not something with index/workdir sync semantics).
pub fn reset_to(repo: &Repository, target: &str, mode: ResetMode) -> PushGitResult<()> {
    let object = repo.revparse_single(target)?;
    let reset_type = match mode {
        ResetMode::Soft => ResetType::Soft,
        ResetMode::Mixed => ResetType::Mixed,
        ResetMode::Hard => ResetType::Hard,
    };
    repo.reset(&object, reset_type, None)?;
    Ok(())
}

fn collect_conflict_paths(index: &Index) -> PushGitResult<Vec<String>> {
    let mut paths = Vec::new();
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let entry = conflict.our.or(conflict.their).or(conflict.ancestor);
        if let Some(path) = entry.and_then(|e| String::from_utf8(e.path).ok()) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn invalid(message: impl Into<String>) -> PushGitError {
    PushGitError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;
    use std::path::Path;

    fn commit_file(repo: &Repository, name: &str, content: &str) -> git2::Oid {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &[&head])
            .unwrap()
    }

    #[test]
    fn create_checkout_rename_and_delete_a_branch() {
        let (_dir, repo) = repo_init();

        create_branch(&repo, "feature", None).unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());

        checkout_branch(&repo, "feature").unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));

        rename_branch(&repo, "feature", "feature-renamed").unwrap();
        assert!(repo
            .find_branch("feature-renamed", BranchType::Local)
            .is_ok());

        checkout_branch(&repo, "main").unwrap();
        delete_branch(&repo, "feature-renamed").unwrap();
        assert!(repo
            .find_branch("feature-renamed", BranchType::Local)
            .is_err());
    }

    #[test]
    fn checkout_commit_detaches_head_at_the_given_oid() {
        let (_dir, repo) = repo_init();
        let base_oid = repo.head().unwrap().target().unwrap();
        let tip_oid = commit_file(&repo, "a.txt", "a\n");

        checkout_commit(&repo, &base_oid.to_string()).unwrap();

        assert!(repo.head_detached().unwrap());
        assert_eq!(repo.head().unwrap().target().unwrap(), base_oid);
        assert_ne!(base_oid, tip_oid);
    }

    #[test]
    fn checkout_remote_branch_creates_a_tracking_branch_when_none_exists_locally() {
        let (dir, repo) = repo_init();
        let remote_path = dir.path().to_str().unwrap();
        repo.remote("origin", remote_path).unwrap();
        let oid = repo.head().unwrap().target().unwrap();
        repo.reference("refs/remotes/origin/feature", oid, true, "")
            .unwrap();

        checkout_remote_branch(&repo, "origin/feature").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));
        let branch = repo.find_branch("feature", BranchType::Local).unwrap();
        let upstream = branch.upstream().unwrap();
        assert_eq!(upstream.name().unwrap(), Some("origin/feature"));
    }

    #[test]
    fn checkout_remote_branch_reuses_an_existing_local_branch_of_the_same_name() {
        let (dir, repo) = repo_init();
        let remote_path = dir.path().to_str().unwrap();
        repo.remote("origin", remote_path).unwrap();
        let oid = repo.head().unwrap().target().unwrap();
        repo.reference("refs/remotes/origin/feature", oid, true, "")
            .unwrap();
        create_branch(&repo, "feature", None).unwrap();

        checkout_remote_branch(&repo, "origin/feature").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));
        let branch = repo.find_branch("feature", BranchType::Local).unwrap();
        assert!(
            branch.upstream().is_err(),
            "should not set an upstream on a pre-existing local branch"
        );
    }

    #[test]
    fn checkout_remote_branch_errors_for_a_malformed_name_without_a_slash() {
        let (_dir, repo) = repo_init();
        assert!(checkout_remote_branch(&repo, "feature").is_err());
    }

    #[test]
    fn repo_state_is_clean_by_default() {
        let (_dir, repo) = repo_init();
        assert_eq!(repo_state(&repo), RepoState::Clean);
    }

    #[test]
    fn repo_state_is_merge_during_a_conflicted_merge() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();

        merge_branch(&repo, "feature").unwrap();

        assert_eq!(repo_state(&repo), RepoState::Merge);
    }

    #[test]
    fn repo_state_is_rebase_during_a_conflicted_rebase() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");

        rebase_branch(&repo, "main").unwrap();

        assert_eq!(repo_state(&repo), RepoState::Rebase);
    }

    #[test]
    fn reset_soft_moves_head_but_keeps_the_index_and_workdir() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().target().unwrap();
        commit_file(&repo, "a.txt", "a\n");

        reset_to(&repo, &base.to_string(), ResetMode::Soft).unwrap();

        assert_eq!(repo.head().unwrap().target().unwrap(), base);
        // Soft reset leaves the previous tip's changes staged.
        let diffs = crate::diff::diff_staged(&repo).unwrap();
        assert!(diffs.iter().any(|d| d.new_path.as_deref() == Some("a.txt")));
        assert!(dir.path().join("a.txt").exists());
    }

    #[test]
    fn reset_mixed_moves_head_and_unstages_but_keeps_the_workdir() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().target().unwrap();
        commit_file(&repo, "a.txt", "a\n");

        reset_to(&repo, &base.to_string(), ResetMode::Mixed).unwrap();

        assert_eq!(repo.head().unwrap().target().unwrap(), base);
        let staged = crate::diff::diff_staged(&repo).unwrap();
        assert!(staged.is_empty());
        assert!(
            dir.path().join("a.txt").exists(),
            "mixed reset keeps the working tree"
        );
    }

    #[test]
    fn reset_hard_moves_head_and_discards_everything() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().target().unwrap();
        commit_file(&repo, "a.txt", "a\n");

        reset_to(&repo, &base.to_string(), ResetMode::Hard).unwrap();

        assert_eq!(repo.head().unwrap().target().unwrap(), base);
        assert!(!dir.path().join("a.txt").exists());
    }

    #[test]
    fn ahead_behind_counts_reflect_divergence_from_upstream() {
        let (dir, repo) = repo_init();
        // Use the repo itself as its own "remote" for a minimal upstream-tracking test.
        let remote_path = dir.path().to_str().unwrap();
        repo.remote("origin", remote_path).unwrap();
        repo.reference(
            "refs/remotes/origin/main",
            repo.head().unwrap().target().unwrap(),
            true,
            "",
        )
        .unwrap();
        let mut branch = repo.find_branch("main", BranchType::Local).unwrap();
        branch.set_upstream(Some("origin/main")).unwrap();

        commit_file(&repo, "local-only.txt", "local\n");

        let branches = list_branches(&repo).unwrap();
        let main = branches.iter().find(|b| b.name == "main").unwrap();

        assert_eq!(main.upstream.as_deref(), Some("origin/main"));
        assert_eq!(main.ahead, 1);
        assert_eq!(main.behind, 0);
    }

    #[test]
    fn list_branches_marks_only_the_checked_out_branch_as_head() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        let branches = list_branches(&repo).unwrap();
        let main = branches.iter().find(|b| b.name == "main").unwrap();
        let feature = branches.iter().find(|b| b.name == "feature").unwrap();

        assert!(main.is_head, "checked-out branch must report is_head");
        assert!(
            !feature.is_head,
            "every other branch must not report is_head"
        );
    }
}
