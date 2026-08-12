// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Multi-commit cherry-pick — applies a range of commits onto HEAD via real `git cherry-pick
//! <oid>...`, using git's own sequencer for multi-step conflict pausing/resuming
//! (`--continue`/`--abort`). Unlike `branch::cherry_pick`, this is a real libgit2 gap, not a
//! design choice: git2-rs's cherry-pick API only ever applies one commit at a time with no
//! sequencer/queue concept at all, so there is nothing to build a multi-commit sequence on
//! top of in-process — `git2::RepositoryState::CherryPickSequence` exists as a state git2
//! can *report*, but nothing in git2 can *drive* one.
//!
//! `--continue` here behaves differently from `branch::cherry_pick`'s conflict recovery:
//! real `git cherry-pick --continue` both commits the resolved step *and* advances to the
//! next queued commit itself, so (unlike the single-commit path, which reuses the ordinary
//! staging/commit flow via `stage::commit`'s `CHERRY_PICK_HEAD` detection) this needs its
//! own dedicated continue/abort commands — see `branch::is_multi_cherry_pick_in_progress`
//! for how the frontend tells the two recovery paths apart.

use std::path::Path;
use std::process::Output;

use crate::branch::{self, CherryPickOutcome};
use crate::error::{PushGitError, PushGitResult};
use crate::remote::run_git_capturing_output;
use crate::repo::{git_dir_of, workdir_of};

/// `GIT_EDITOR=true` accepts each step's own commit message unedited — this range never
/// offers a custom per-commit message (matching `branch::cherry_pick`'s own "preserve the
/// original message" default), so there's nothing for an interactive editor to add. Passed
/// to every call below (cherry-pick, `--continue`, `--abort` alike), matching how the
/// original single `run_subprocess` wrapper applied it unconditionally.
const CHERRY_PICK_ENV: &[(&str, &str)] = &[("GIT_EDITOR", "true")];

/// Classifies a `git cherry-pick`/`--continue` subprocess result the same way
/// `interactive_rebase::classify` does for rebase: `CherryPicked` if it exited successfully
/// and nothing is left in progress, `Conflicts` if `.git/CHERRY_PICK_HEAD` is still there,
/// otherwise a genuine error.
fn classify(repo_path: &Path, output: &Output) -> PushGitResult<CherryPickOutcome> {
    let cherry_pick_head = git_dir_of(repo_path)?.join("CHERRY_PICK_HEAD");
    if output.status.success() && !cherry_pick_head.exists() {
        let repo = crate::repo::open(repo_path)?;
        let oid = repo
            .head()?
            .target()
            .ok_or_else(|| PushGitError::Invalid("HEAD has no target".to_string()))?;
        Ok(CherryPickOutcome::CherryPicked {
            oid: oid.to_string(),
        })
    } else if cherry_pick_head.exists() {
        let repo = crate::repo::open(repo_path)?;
        Ok(CherryPickOutcome::Conflicts {
            conflicts: branch::list_conflicts(&repo)?,
        })
    } else {
        Err(PushGitError::Subprocess {
            command: "git cherry-pick".to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Applies `commit_oids`, in the given order, onto HEAD — stops at the first conflict.
pub async fn cherry_pick_range(
    repo_path: &Path,
    commit_oids: &[String],
) -> PushGitResult<CherryPickOutcome> {
    let workdir = workdir_of(repo_path)?;
    let mut args = vec!["cherry-pick"];
    args.extend(commit_oids.iter().map(String::as_str));

    let output = run_git_capturing_output(&args, &workdir, CHERRY_PICK_ENV).await?;
    classify(repo_path, &output)
}

/// Resumes a cherry-pick range paused by a conflict, once the caller has resolved and
/// staged it — mirrors `git cherry-pick --continue`, which both commits the resolved step
/// and advances to the next queued commit.
pub async fn continue_cherry_pick_range(repo_path: &Path) -> PushGitResult<CherryPickOutcome> {
    let workdir = workdir_of(repo_path)?;
    let output =
        run_git_capturing_output(&["cherry-pick", "--continue"], &workdir, CHERRY_PICK_ENV).await?;
    classify(repo_path, &output)
}

/// Aborts an in-progress cherry-pick range, restoring HEAD to where it was before
/// `cherry_pick_range` ran — mirrors `git cherry-pick --abort`.
pub async fn abort_cherry_pick_range(repo_path: &Path) -> PushGitResult<()> {
    let workdir = workdir_of(repo_path)?;
    let output =
        run_git_capturing_output(&["cherry-pick", "--abort"], &workdir, CHERRY_PICK_ENV).await?;
    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: "git cherry-pick --abort".to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch};
    use crate::test_support::repo_init;
    use std::fs;

    fn commit_file(repo: &git2::Repository, name: &str, content: &str) -> git2::Oid {
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

    #[tokio::test]
    async fn applies_a_clean_range_of_commits_in_order() {
        let (dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        let a = commit_file(&repo, "a.txt", "a\n");
        let b = commit_file(&repo, "b.txt", "b\n");
        checkout_branch(&repo, "main").unwrap();

        let outcome = cherry_pick_range(dir.path(), &[a.to_string(), b.to_string()])
            .await
            .unwrap();

        assert!(matches!(outcome, CherryPickOutcome::CherryPicked { .. }));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "b.txt");
        assert_eq!(head.parent(0).unwrap().message().unwrap(), "a.txt");
    }

    #[tokio::test]
    async fn a_conflicting_step_pauses_as_a_sequence_and_can_be_continued() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        let conflicting = commit_file(&repo, "shared.txt", "feature version\n");
        let clean = commit_file(&repo, "other.txt", "other\n");
        checkout_branch(&repo, "main").unwrap();
        commit_file(&repo, "shared.txt", "main version\n");

        let outcome = cherry_pick_range(dir.path(), &[conflicting.to_string(), clean.to_string()])
            .await
            .unwrap();

        let conflicts = match outcome {
            CherryPickOutcome::Conflicts { conflicts } => conflicts,
            other => panic!("expected conflicts, got {other:?}"),
        };
        assert_eq!(conflicts, vec!["shared.txt".to_string()]);

        let repo = crate::repo::open(dir.path()).unwrap();
        assert!(branch::is_multi_cherry_pick_in_progress(&repo));

        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        drop(index);

        let outcome = continue_cherry_pick_range(dir.path()).await.unwrap();

        assert!(matches!(outcome, CherryPickOutcome::CherryPicked { .. }));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "other.txt");
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "resolved\n"
        );
        assert_eq!(repo.state(), git2::RepositoryState::Clean);
    }

    #[tokio::test]
    async fn abort_restores_the_original_head() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        let conflicting = commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        let original_head = repo.head().unwrap().target().unwrap();

        let outcome = cherry_pick_range(dir.path(), &[conflicting.to_string()])
            .await
            .unwrap();
        assert!(matches!(outcome, CherryPickOutcome::Conflicts { .. }));

        abort_cherry_pick_range(dir.path()).await.unwrap();

        let repo = crate::repo::open(dir.path()).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap(), original_head);
        assert_eq!(repo.state(), git2::RepositoryState::Clean);
    }
}
