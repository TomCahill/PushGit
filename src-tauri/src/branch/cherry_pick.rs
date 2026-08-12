// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Single-commit cherry-pick via git2's own cherry-pick support — no libgit2 gap here,
//! unlike `cherry_pick_range/`'s multi-commit sequencer path.

use git2::{Repository, ResetType};

use super::collect_conflict_paths;
use super::model::CherryPickOutcome;
use crate::error::PushGitResult;

/// Applies `commit_oid`'s changes onto HEAD as a new commit, preserving the original
/// author (matching plain `git cherry-pick`'s default behavior) with the current user as
/// committer. Conflicts are left staged in the index, exactly like `merge_branch`, for the
/// caller to resolve via the ordinary staging/commit flow — `stage::commit` already detects
/// `CHERRY_PICK_HEAD` and finishes it correctly (single parent, original author) rather than
/// needing its own dedicated "continue" command.
pub fn cherry_pick(repo: &Repository, commit_oid: &str) -> PushGitResult<CherryPickOutcome> {
    let commit = repo.find_commit(repo.revparse_single(commit_oid)?.id())?;
    repo.cherrypick(&commit, None)?;

    let mut index = repo.index()?;
    if index.has_conflicts() {
        return Ok(CherryPickOutcome::Conflicts {
            conflicts: collect_conflict_paths(&index)?,
        });
    }

    let tree = repo.find_tree(index.write_tree()?)?;
    let committer = repo.signature()?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let message = commit.message().unwrap_or_default();

    let oid = repo.commit(
        Some("HEAD"),
        &commit.author(),
        &committer,
        message,
        &tree,
        &[&head_commit],
    )?;
    repo.cleanup_state()?;

    Ok(CherryPickOutcome::CherryPicked {
        oid: oid.to_string(),
    })
}

/// Aborts an in-progress cherry-pick, discarding the staged conflict resolution and
/// restoring the working tree to HEAD — mirrors `abort_merge`.
pub fn abort_cherry_pick(repo: &Repository) -> PushGitResult<()> {
    let head_commit = repo.head()?.peel_to_commit()?;
    repo.reset(head_commit.as_object(), ResetType::Hard, None)?;
    repo.cleanup_state()?;
    Ok(())
}

/// Whether a paused cherry-pick (`repo_state() == RepoState::CherryPick`) is a multi-commit
/// range (`cherry_pick_range::cherry_pick_range`, real `git cherry-pick <oid>...` under the
/// hood) rather than `cherry_pick`'s single-commit git2 path — `BranchSidebar` needs this to
/// offer the right recovery action, since a range's conflict is resolved via `git
/// cherry-pick --continue` (which commits *and* advances to the next queued commit itself),
/// not the ordinary staging/commit flow `cherry_pick`'s own conflicts use. Unlike
/// `interactive_rebase`'s equivalent check, this needs no custom marker file — libgit2
/// already reports a distinct `RepositoryState::CherryPickSequence` (backed by
/// `.git/sequencer/`) once more than one commit is queued, separate from plain
/// `RepositoryState::CherryPick`.
pub fn is_multi_cherry_pick_in_progress(repo: &Repository) -> bool {
    matches!(repo.state(), git2::RepositoryState::CherryPickSequence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch};
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
    fn is_multi_cherry_pick_in_progress_is_false_outside_a_paused_sequence() {
        // The `true` case (a real, paused multi-commit sequence via `git cherry-pick
        // <oid>...`) is covered by `cherry_pick_range::tests::
        // a_conflicting_step_pauses_as_a_sequence_and_can_be_continued` — this only needs
        // the ordinary, not-in-progress case, which is otherwise never asserted.
        let (_dir, repo) = repo_init();
        assert!(!is_multi_cherry_pick_in_progress(&repo));
    }

    #[test]
    fn cherry_pick_applies_a_commit_onto_head_preserving_its_author() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();

        fs::write(repo.workdir().unwrap().join("a.txt"), "a\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let author = git2::Signature::now("Ada Lovelace", "ada@example.com").unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let feature_commit = repo
            .commit(Some("HEAD"), &author, &author, "a.txt", &tree, &[&head])
            .unwrap();

        checkout_branch(&repo, "main").unwrap();

        let outcome = cherry_pick(&repo, &feature_commit.to_string()).unwrap();

        let CherryPickOutcome::CherryPicked { oid } = outcome else {
            panic!("expected a clean cherry-pick, got {outcome:?}");
        };
        let new_commit = repo
            .find_commit(git2::Oid::from_str(&oid).unwrap())
            .unwrap();
        assert_eq!(new_commit.parent_count(), 1);
        assert_eq!(new_commit.author().name(), Some("Ada Lovelace"));
        assert_eq!(
            crate::branch::repo_state(&repo),
            crate::branch::RepoState::Clean
        );
    }

    #[test]
    fn cherry_pick_reports_conflicts_and_can_be_aborted() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        let feature_commit = commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        let main_tip = commit_file(&repo, "shared.txt", "main version\n");

        let outcome = cherry_pick(&repo, &feature_commit.to_string()).unwrap();

        match outcome {
            CherryPickOutcome::Conflicts { conflicts } => {
                assert_eq!(conflicts, vec!["shared.txt".to_string()])
            }
            other => panic!("expected conflicts, got {other:?}"),
        }
        assert_eq!(
            crate::branch::repo_state(&repo),
            crate::branch::RepoState::CherryPick
        );

        abort_cherry_pick(&repo).unwrap();

        assert_eq!(
            crate::branch::repo_state(&repo),
            crate::branch::RepoState::Clean
        );
        assert_eq!(repo.head().unwrap().target().unwrap(), main_tip);
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "main version\n"
        );
    }
}
