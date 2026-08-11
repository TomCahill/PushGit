// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fast-forward and real 3-way merge, plus abort.

use git2::{BranchType, Repository, ResetType};

use super::model::MergeOutcome;
use super::{collect_conflict_paths, invalid};
use crate::error::PushGitResult;

/// Merges `branch_name` into the current HEAD, taking the fast-forward path when possible
/// and otherwise performing a real 3-way merge. Conflicts are left staged in the index
/// (mirroring plain `git merge`'s behavior) for the caller to resolve via
/// `list_conflicts`/`resolve_conflict`, not auto-aborted.
pub fn merge_branch(repo: &Repository, branch_name: &str) -> PushGitResult<MergeOutcome> {
    let their_oid = resolve_branch_oid(repo, branch_name)?;
    let their_annotated = repo.find_annotated_commit(their_oid)?;

    let (analysis, _preference) = repo.merge_analysis(&[&their_annotated])?;

    if analysis.is_up_to_date() {
        return Ok(MergeOutcome::AlreadyUpToDate);
    }

    if analysis.is_fast_forward() {
        let mut head_ref = repo.head()?;
        head_ref.set_target(their_oid, "pushgit: fast-forward merge")?;
        repo.set_head(
            head_ref
                .name()
                .ok_or_else(|| invalid("HEAD ref has no name"))?,
        )?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        return Ok(MergeOutcome::FastForward);
    }

    commit_merge(repo, branch_name, their_oid, &their_annotated)
}

/// Like `merge_branch`, but never fast-forwards — always leaves a real two-parent merge
/// commit (mirrors `git merge --no-ff`), so GitFlow release/hotfix finishes visibly show
/// the branch as a fork+merge in the graph even when a fast-forward was possible.
pub fn merge_branch_no_ff(repo: &Repository, branch_name: &str) -> PushGitResult<MergeOutcome> {
    let their_oid = resolve_branch_oid(repo, branch_name)?;
    let their_annotated = repo.find_annotated_commit(their_oid)?;

    let (analysis, _preference) = repo.merge_analysis(&[&their_annotated])?;

    if analysis.is_up_to_date() {
        return Ok(MergeOutcome::AlreadyUpToDate);
    }

    commit_merge(repo, branch_name, their_oid, &their_annotated)
}

fn commit_merge(
    repo: &Repository,
    branch_name: &str,
    their_oid: git2::Oid,
    their_annotated: &git2::AnnotatedCommit<'_>,
) -> PushGitResult<MergeOutcome> {
    repo.merge(&[their_annotated], None, None)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(MergeOutcome::Conflicts(collect_conflict_paths(&index)?));
    }

    let tree = repo.find_tree(index.write_tree()?)?;
    let signature = repo.signature()?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let their_commit = repo.find_commit(their_oid)?;
    let message = format!("Merge branch '{branch_name}'");
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &message,
        &tree,
        &[&head_commit, &their_commit],
    )?;
    repo.cleanup_state()?;

    Ok(MergeOutcome::Merged)
}

/// Aborts an in-progress merge, discarding the merge state and any staged conflict
/// resolutions, and restoring the working tree to HEAD.
pub fn abort_merge(repo: &Repository) -> PushGitResult<()> {
    let head_commit = repo.head()?.peel_to_commit()?;
    repo.reset(head_commit.as_object(), ResetType::Hard, None)?;
    repo.cleanup_state()?;
    Ok(())
}

fn resolve_branch_oid(repo: &Repository, name: &str) -> PushGitResult<git2::Oid> {
    let branch = repo
        .find_branch(name, BranchType::Local)
        .or_else(|_| repo.find_branch(name, BranchType::Remote))?;
    branch
        .get()
        .target()
        .ok_or_else(|| invalid(format!("branch '{name}' has no target")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch, list_conflicts};
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
    fn fast_forward_merge_moves_head_without_a_merge_commit() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        let tip = commit_file(&repo, "a.txt", "a\n");

        checkout_branch(&repo, "main").unwrap();
        let outcome = merge_branch(&repo, "feature").unwrap();

        assert!(matches!(outcome, MergeOutcome::FastForward));
        assert_eq!(repo.head().unwrap().target().unwrap(), tip);
    }

    #[test]
    fn merging_an_already_merged_branch_reports_up_to_date() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        let outcome = merge_branch(&repo, "feature").unwrap();

        assert!(matches!(outcome, MergeOutcome::AlreadyUpToDate));
    }

    #[test]
    fn diverging_branches_merge_with_a_real_merge_commit() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        commit_file(&repo, "main-only.txt", "main\n");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "feature-only.txt", "feature\n");

        checkout_branch(&repo, "main").unwrap();
        let outcome = merge_branch(&repo, "feature").unwrap();

        assert!(matches!(outcome, MergeOutcome::Merged));
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_count(), 2);
    }

    #[test]
    fn conflicting_merge_reports_conflicted_paths_and_can_be_aborted() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();

        commit_file(&repo, "shared.txt", "main version\n");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");

        checkout_branch(&repo, "main").unwrap();
        let outcome = merge_branch(&repo, "feature").unwrap();

        match outcome {
            MergeOutcome::Conflicts(paths) => assert_eq!(paths, vec!["shared.txt".to_string()]),
            other => panic!("expected conflicts, got {other:?}"),
        }

        assert_eq!(
            list_conflicts(&repo).unwrap(),
            vec!["shared.txt".to_string()]
        );

        abort_merge(&repo).unwrap();
        assert!(list_conflicts(&repo).unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "main version\n"
        );
    }
}
