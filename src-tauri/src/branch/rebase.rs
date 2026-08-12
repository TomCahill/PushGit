// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Non-interactive rebase via git2's own cherry-pick-based `Rebase` API (the
//! `rebase-merge` machinery libgit2 supports natively) — only *interactive* rebase
//! (reorder/squash/reword) is a hard gap requiring the real `git` binary; see
//! `interactive_rebase/`.

use git2::Repository;

use super::collect_conflict_paths;
use super::model::RebaseOutcome;
use crate::error::PushGitResult;

/// Non-interactive rebase of the current branch onto `onto` — applies each commit via
/// git2's own cherry-pick-based rebase machinery, stopping at the first conflict.
pub fn rebase_branch(repo: &Repository, onto: &str) -> PushGitResult<RebaseOutcome> {
    let onto_commit = repo.revparse_single(onto)?.peel_to_commit()?;
    let onto_annotated = repo.find_annotated_commit(onto_commit.id())?;

    let mut rebase = repo.rebase(None, None, Some(&onto_annotated), None)?;

    while let Some(step) = rebase.next() {
        step?;

        let index = repo.index()?;
        if index.has_conflicts() {
            let conflicts = collect_conflict_paths(&index)?;
            return Ok(RebaseOutcome::Conflicts(conflicts));
        }

        let signature = repo.signature()?;
        rebase.commit(None, &signature, None)?;
    }

    rebase.finish(None)?;
    Ok(RebaseOutcome::Completed)
}

pub fn abort_rebase(repo: &Repository) -> PushGitResult<()> {
    let mut rebase = repo.open_rebase(None)?;
    rebase.abort()?;
    Ok(())
}

/// Resumes a rebase paused by a conflict (mirrors `git rebase --continue`) — the caller
/// must have already resolved and staged the conflicted paths. Commits the step that was
/// left pending, then replays any remaining commits exactly as `rebase_branch` does,
/// stopping again at the next conflict if there is one.
pub fn continue_rebase(repo: &Repository) -> PushGitResult<RebaseOutcome> {
    let mut rebase = repo.open_rebase(None)?;
    let signature = repo.signature()?;

    // The operation `rebase_branch`'s loop last returned from `next()` is still pending —
    // it must be committed before advancing, per libgit2's rebase contract.
    rebase.commit(None, &signature, None)?;

    while let Some(step) = rebase.next() {
        step?;

        let index = repo.index()?;
        if index.has_conflicts() {
            return Ok(RebaseOutcome::Conflicts(collect_conflict_paths(&index)?));
        }

        rebase.commit(None, &signature, None)?;
    }

    rebase.finish(None)?;
    Ok(RebaseOutcome::Completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch, list_conflicts, resolve_conflict};
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
    fn non_interactive_rebase_replays_commits_onto_the_target() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        commit_file(&repo, "main-only.txt", "main\n");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "feature-only.txt", "feature\n");

        let outcome = rebase_branch(&repo, "main").unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "feature-only.txt");
        let parent = head.parent(0).unwrap();
        assert_eq!(parent.message().unwrap(), "main-only.txt");
    }

    #[test]
    fn a_conflicting_rebase_can_be_aborted_back_to_the_original_head() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();

        commit_file(&repo, "shared.txt", "main version\n");

        checkout_branch(&repo, "feature").unwrap();
        let feature_tip = commit_file(&repo, "shared.txt", "feature version\n");

        let outcome = rebase_branch(&repo, "main").unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflicts(_)));

        abort_rebase(&repo).unwrap();

        assert!(list_conflicts(&repo).unwrap().is_empty());
        assert_eq!(repo.head().unwrap().target().unwrap(), feature_tip);
        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));
    }

    #[test]
    fn continue_rebase_finishes_after_the_conflict_is_resolved() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();

        let main_tip = commit_file(&repo, "shared.txt", "main version\n");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");

        let outcome = rebase_branch(&repo, "main").unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflicts(_)));

        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        resolve_conflict(&repo, "shared.txt").unwrap();

        let outcome = continue_rebase(&repo).unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        assert!(list_conflicts(&repo).unwrap().is_empty());
        assert_eq!(
            crate::branch::repo_state(&repo),
            crate::branch::RepoState::Clean
        );
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent_id(0).unwrap(), main_tip);
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "resolved\n"
        );
    }

    #[test]
    fn continue_rebase_can_hit_a_second_conflict_on_a_later_commit() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "f1.txt", "base1\n");
        commit_file(&repo, "f2.txt", "base2\n");
        create_branch(&repo, "feature", None).unwrap();

        commit_file(&repo, "f1.txt", "main1\n");
        let main_tip = commit_file(&repo, "f2.txt", "main2\n");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "f1.txt", "feature1\n");
        commit_file(&repo, "f2.txt", "feature2\n");

        let outcome = rebase_branch(&repo, "main").unwrap();
        match outcome {
            RebaseOutcome::Conflicts(paths) => assert_eq!(paths, vec!["f1.txt".to_string()]),
            other => panic!("expected conflicts on f1.txt, got {other:?}"),
        }

        fs::write(dir.path().join("f1.txt"), "f1 resolved\n").unwrap();
        resolve_conflict(&repo, "f1.txt").unwrap();

        let outcome = continue_rebase(&repo).unwrap();
        match outcome {
            RebaseOutcome::Conflicts(paths) => assert_eq!(paths, vec!["f2.txt".to_string()]),
            other => panic!("expected conflicts on f2.txt, got {other:?}"),
        }

        fs::write(dir.path().join("f2.txt"), "f2 resolved\n").unwrap();
        resolve_conflict(&repo, "f2.txt").unwrap();

        let outcome = continue_rebase(&repo).unwrap();
        assert!(matches!(outcome, RebaseOutcome::Completed));
        assert_eq!(
            crate::branch::repo_state(&repo),
            crate::branch::RepoState::Clean
        );

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.parent(0).unwrap().parent_id(0).unwrap(), main_tip);
        assert_eq!(
            fs::read_to_string(dir.path().join("f1.txt")).unwrap(),
            "f1 resolved\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("f2.txt")).unwrap(),
            "f2 resolved\n"
        );
    }
}
