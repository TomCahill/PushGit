// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Interactive rebase (reorder/squash/reword/drop) — a thin UI over real `git rebase -i`
//! sequencing via a scripted `GIT_SEQUENCE_EDITOR`/`GIT_EDITOR`, not a libgit2
//! reimplementation: libgit2 only implements the
//! cherry-pick-based `rebase-merge` machinery, not `rebase-interactive`'s todo-editing UI
//! (tracked upstream in libgit2 issue #6332, open since June 2022).
//!
//! **How the scripting works.** `git rebase -i` normally opens `$GIT_SEQUENCE_EDITOR` on a
//! generated todo file so a human can edit it, then (for `reword`/`squash` steps)
//! `$GIT_EDITOR` on a commit-message file per step. Both env vars accept an arbitrary
//! command, which git invokes via `sh -c "$VAR \"$file\""` — so this module never lets any
//! caller-supplied text (a commit message) become part of that shell-interpreted command
//! string. Instead: `GIT_SEQUENCE_EDITOR` is the fixed, literal `cp .pushgit-rebase-todo`
//! (git appends its own scratch-file path as a quoted argument), and reword messages are
//! written to their own transient files first, referenced from an `exec git commit --amend
//! -F <fixed-pattern-filename>` todo line — `-F <file>` takes a file's raw bytes as the
//! message with no shell interpretation of its *content*, and the filename itself is always
//! `.pushgit-reword-<hex-oid>.msg` (hex-only, never containing a shell metacharacter).
//! `GIT_EDITOR=true` makes any `squash` step accept git's own auto-combined message
//! unedited — squash doesn't support a custom combined message in this first version.
//!
//! All the transient files above live at the *worktree root* (not `.git/`), specifically so
//! the todo/exec lines can reference them by a short, always-safe relative filename — `exec`
//! and the sequence editor both run with cwd already set to the worktree root, and a
//! Rust-constructed absolute path could contain spaces the shell-interpreted todo/exec
//! lines would need (but don't get) escaping for. The cost is that a `.pushgit-reword-*.msg`
//! file is briefly visible as an untracked file while a reword step hasn't executed yet —
//! cleaned up automatically once the whole rebase reaches a terminal state.
//!
//! **Continue/abort are their own commands, never `branch::continue_rebase`/`abort_rebase`.**
//! Those use git2's own `Rebase` API; this module always shells out to real `git rebase
//! --continue`/`--abort` instead, since that's the one implementation guaranteed correct for
//! a rebase state `git rebase -i` itself created. The frontend keeps the two conflict-recovery
//! paths separate too — whichever UI started the rebase owns resolving it.

use std::path::{Path, PathBuf};
use std::process::Output;

use git2::Repository;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::branch::{self, RebaseOutcome};
use crate::error::{PushGitError, PushGitResult};

const TODO_FILE: &str = ".pushgit-rebase-todo";
const REWORD_PREFIX: &str = ".pushgit-reword-";
const REWORD_SUFFIX: &str = ".msg";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseCommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebaseAction {
    Pick,
    Squash,
    Reword { message: String },
    Drop,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebaseStep {
    pub oid: String,
    pub action: RebaseAction,
}

/// The commits between `onto` and HEAD, oldest first — the order an interactive rebase's
/// todo list uses, and the order the frontend's reorderable list should default to before
/// the user drags anything.
pub fn list_rebase_commits(
    repo: &Repository,
    onto: &str,
) -> PushGitResult<Vec<RebaseCommitSummary>> {
    let onto_oid = repo.revparse_single(onto)?.peel_to_commit()?.id();
    let head_oid = repo.head()?.peel_to_commit()?.id();

    let mut revwalk = repo.revwalk()?;
    revwalk.push(head_oid)?;
    revwalk.hide(onto_oid)?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;

    let mut commits = Vec::new();
    for oid in revwalk {
        let commit = repo.find_commit(oid?)?;
        let oid_str = commit.id().to_string();
        let short_oid = oid_str[..7.min(oid_str.len())].to_string();
        commits.push(RebaseCommitSummary {
            oid: oid_str,
            short_oid,
            summary: commit.summary().unwrap_or_default().to_string(),
        });
    }
    commits.reverse(); // revwalk yields newest-first; todo order is oldest-first
    Ok(commits)
}

fn reword_filename(oid: &str) -> String {
    format!("{REWORD_PREFIX}{oid}{REWORD_SUFFIX}")
}

/// Builds the `git-rebase-todo` content for `steps`, in the order they should be applied,
/// writing any reword messages to their own transient files first (see the module doc for
/// why messages never become part of the todo file's own text).
fn build_todo(workdir: &Path, steps: &[RebaseStep]) -> PushGitResult<String> {
    let mut lines = Vec::new();
    for step in steps {
        match &step.action {
            RebaseAction::Pick => lines.push(format!("pick {}", step.oid)),
            RebaseAction::Squash => lines.push(format!("squash {}", step.oid)),
            RebaseAction::Drop => lines.push(format!("drop {}", step.oid)),
            RebaseAction::Reword { message } => {
                let filename = reword_filename(&step.oid);
                std::fs::write(workdir.join(&filename), message)?;
                lines.push(format!("pick {}", step.oid));
                lines.push(format!("exec git commit --amend -F {filename}"));
            }
        }
    }
    Ok(lines.join("\n") + "\n")
}

fn workdir_of(repo_path: &Path) -> PushGitResult<PathBuf> {
    crate::repo::open(repo_path)?
        .workdir()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| PushGitError::Invalid("repository has no working directory".to_string()))
}

fn git_dir_of(repo_path: &Path) -> PushGitResult<PathBuf> {
    Ok(crate::repo::open(repo_path)?.path().to_path_buf())
}

async fn run_rebase_subprocess(
    args: &[&str],
    cwd: &Path,
    extra_env: &[(&str, &str)],
) -> PushGitResult<Output> {
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .output()
        .await
        .map_err(|e| PushGitError::Subprocess {
            command: format!("git {}", args.join(" ")),
            message: e.to_string(),
        })
}

/// `.git/rebase-merge` is the *same* on-disk format a plain (non-interactive) rebase uses
/// (`branch::rebase_branch`, via git2's own `Rebase` API) — so a paused interactive rebase
/// is otherwise indistinguishable from a paused plain one to anything that only checks
/// `branch::repo_state()`. This marker, written the moment this module's own rebase pauses,
/// is what lets the frontend tell `BranchSidebar`'s plain-rebase conflict banner (whose
/// Continue/Abort call the git2-based `branch::continue_rebase`/`abort_rebase`) not to offer
/// its own recovery controls for a state only this module's `continue_interactive_rebase`/
/// `abort_interactive_rebase` should ever touch — see the module doc's opening paragraph.
const INTERACTIVE_MARKER: &str = "PUSHGIT_INTERACTIVE_REBASE";

/// Whether `.git/rebase-merge` (if any) was left there by this module rather than
/// `branch::rebase_branch` — see `INTERACTIVE_MARKER`.
pub fn is_interactive_rebase_in_progress(repo_path: &Path) -> PushGitResult<bool> {
    Ok(git_dir_of(repo_path)?
        .join("rebase-merge")
        .join(INTERACTIVE_MARKER)
        .exists())
}

/// Classifies a rebase subprocess's result: `Completed` if it exited successfully and no
/// rebase is left in progress, `Conflicts` if `.git/rebase-merge` is still there (paused —
/// whether by a real merge conflict or an `exec` step failing), otherwise a genuine error.
/// Stamps `INTERACTIVE_MARKER` into it whenever it's still there, so a resumed-then-paused-
/// again rebase (a second conflicting step after `--continue`) stays marked too.
fn classify(repo_path: &Path, output: &Output) -> PushGitResult<RebaseOutcome> {
    let rebase_merge = git_dir_of(repo_path)?.join("rebase-merge");
    if output.status.success() && !rebase_merge.exists() {
        Ok(RebaseOutcome::Completed)
    } else if rebase_merge.exists() {
        let _ = std::fs::write(rebase_merge.join(INTERACTIVE_MARKER), b"");
        let repo = crate::repo::open(repo_path)?;
        Ok(RebaseOutcome::Conflicts(branch::list_conflicts(&repo)?))
    } else {
        Err(PushGitError::Subprocess {
            command: "git rebase".to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn cleanup_transient_files(workdir: &Path) {
    let _ = std::fs::remove_file(workdir.join(TODO_FILE));
    let Ok(entries) = std::fs::read_dir(workdir) else {
        return;
    };
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if name.starts_with(REWORD_PREFIX) && name.ends_with(REWORD_SUFFIX) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Starts (and, whenever nothing conflicts, finishes) an interactive rebase of the current
/// branch onto `onto`, replaying `steps` in the given order.
pub async fn start_interactive_rebase(
    repo_path: &Path,
    onto: &str,
    steps: &[RebaseStep],
) -> PushGitResult<RebaseOutcome> {
    let workdir = workdir_of(repo_path)?;

    let todo = build_todo(&workdir, steps)?;
    std::fs::write(workdir.join(TODO_FILE), todo)?;

    let output = run_rebase_subprocess(
        &["rebase", "-i", onto],
        &workdir,
        &[
            ("GIT_SEQUENCE_EDITOR", "cp .pushgit-rebase-todo"),
            ("GIT_EDITOR", "true"),
        ],
    )
    .await?;

    let outcome = classify(repo_path, &output)?;
    if matches!(outcome, RebaseOutcome::Completed) {
        cleanup_transient_files(&workdir);
    }
    Ok(outcome)
}

/// Resumes an interactive rebase paused by a conflict, once the caller has resolved and
/// staged it — mirrors `git rebase --continue`.
pub async fn continue_interactive_rebase(repo_path: &Path) -> PushGitResult<RebaseOutcome> {
    let workdir = workdir_of(repo_path)?;
    let output = run_rebase_subprocess(
        &["rebase", "--continue"],
        &workdir,
        &[("GIT_EDITOR", "true")],
    )
    .await?;

    let outcome = classify(repo_path, &output)?;
    if matches!(outcome, RebaseOutcome::Completed) {
        cleanup_transient_files(&workdir);
    }
    Ok(outcome)
}

/// Aborts an in-progress interactive rebase, restoring the branch to where it was before
/// `start_interactive_rebase` ran — mirrors `git rebase --abort`.
pub async fn abort_interactive_rebase(repo_path: &Path) -> PushGitResult<()> {
    let workdir = workdir_of(repo_path)?;
    let output = run_rebase_subprocess(&["rebase", "--abort"], &workdir, &[]).await?;
    cleanup_transient_files(&workdir);

    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: "git rebase --abort".to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;

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
    fn list_rebase_commits_returns_oldest_first() {
        let (_dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        commit_file(&repo, "a.txt", "a\n");
        commit_file(&repo, "b.txt", "b\n");

        let commits = list_rebase_commits(&repo, &base.to_string()).unwrap();

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].summary, "a.txt");
        assert_eq!(commits[1].summary, "b.txt");
    }

    #[tokio::test]
    async fn reorders_and_completes_a_clean_rebase() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let a = commit_file(&repo, "a.txt", "a\n");
        let b = commit_file(&repo, "b.txt", "b\n");

        // Swap the order: b before a.
        let steps = vec![
            RebaseStep {
                oid: b.to_string(),
                action: RebaseAction::Pick,
            },
            RebaseStep {
                oid: a.to_string(),
                action: RebaseAction::Pick,
            },
        ];

        let outcome = start_interactive_rebase(dir.path(), &base.to_string(), &steps)
            .await
            .unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "a.txt");
        assert_eq!(head.parent(0).unwrap().message().unwrap(), "b.txt");
    }

    #[tokio::test]
    async fn drops_a_commit() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let a = commit_file(&repo, "a.txt", "a\n");
        let b = commit_file(&repo, "b.txt", "b\n");

        let steps = vec![
            RebaseStep {
                oid: a.to_string(),
                action: RebaseAction::Drop,
            },
            RebaseStep {
                oid: b.to_string(),
                action: RebaseAction::Pick,
            },
        ];

        let outcome = start_interactive_rebase(dir.path(), &base.to_string(), &steps)
            .await
            .unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "b.txt");
        assert_eq!(head.parent_id(0).unwrap(), base);
        assert!(!dir.path().join("a.txt").exists());
    }

    #[tokio::test]
    async fn rewords_a_commit_message() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let a = commit_file(&repo, "a.txt", "a\n");

        let steps = vec![RebaseStep {
            oid: a.to_string(),
            action: RebaseAction::Reword {
                message: "a better message\n".to_string(),
            },
        }];

        let outcome = start_interactive_rebase(dir.path(), &base.to_string(), &steps)
            .await
            .unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(head.message().unwrap(), "a better message\n");
        // No transient files left behind once the rebase completes.
        assert!(!dir.path().join(TODO_FILE).exists());
        assert!(!dir.path().join(reword_filename(&a.to_string())).exists());
    }

    #[tokio::test]
    async fn squashes_two_commits_into_one() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let a = commit_file(&repo, "a.txt", "a\n");
        let b = commit_file(&repo, "b.txt", "b\n");

        let steps = vec![
            RebaseStep {
                oid: a.to_string(),
                action: RebaseAction::Pick,
            },
            RebaseStep {
                oid: b.to_string(),
                action: RebaseAction::Squash,
            },
        ];

        let outcome = start_interactive_rebase(dir.path(), &base.to_string(), &steps)
            .await
            .unwrap();

        assert!(matches!(outcome, RebaseOutcome::Completed));
        let repo = crate::repo::open(dir.path()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(
            head.parent_id(0).unwrap(),
            base,
            "should be a single new commit atop base"
        );
        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("b.txt").exists());
    }

    #[tokio::test]
    async fn a_conflicting_step_pauses_and_can_be_continued() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let feature_commit = commit_file(&repo, "shared.txt", "feature version\n");
        // Diverge `base` itself so replaying `feature_commit` onto it conflicts.
        fs::write(dir.path().join("shared.txt"), "base\n").unwrap();
        {
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new("shared.txt")).unwrap();
            idx.write().unwrap();
        }
        // Reset back to base, then commit a conflicting change directly on top of base.
        repo.reset(
            &repo.find_commit(base).unwrap().into_object(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        commit_file(&repo, "shared.txt", "onto version\n");
        let onto = repo.head().unwrap().peel_to_commit().unwrap().id();

        let steps = vec![RebaseStep {
            oid: feature_commit.to_string(),
            action: RebaseAction::Pick,
        }];

        let outcome = start_interactive_rebase(dir.path(), &onto.to_string(), &steps)
            .await
            .unwrap();

        let conflicts = match outcome {
            RebaseOutcome::Conflicts(paths) => paths,
            other => panic!("expected conflicts, got {other:?}"),
        };
        assert_eq!(conflicts, vec!["shared.txt".to_string()]);
        assert!(
            is_interactive_rebase_in_progress(dir.path()).unwrap(),
            "the paused rebase should be marked as this module's, not branch::rebase_branch's"
        );

        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        {
            let repo = crate::repo::open(dir.path()).unwrap();
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new("shared.txt")).unwrap();
            idx.write().unwrap();
        }

        let outcome = continue_interactive_rebase(dir.path()).await.unwrap();
        assert!(matches!(outcome, RebaseOutcome::Completed));
        let repo = crate::repo::open(dir.path()).unwrap();
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
        let base = repo.head().unwrap().peel_to_commit().unwrap().id();
        let feature_commit = commit_file(&repo, "shared.txt", "feature version\n");
        repo.reset(
            &repo.find_commit(base).unwrap().into_object(),
            git2::ResetType::Hard,
            None,
        )
        .unwrap();
        commit_file(&repo, "shared.txt", "onto version\n");
        let onto = repo.head().unwrap().peel_to_commit().unwrap().id();
        let original_head = repo.head().unwrap().target().unwrap();

        let steps = vec![RebaseStep {
            oid: feature_commit.to_string(),
            action: RebaseAction::Pick,
        }];
        let outcome = start_interactive_rebase(dir.path(), &onto.to_string(), &steps)
            .await
            .unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflicts(_)));

        abort_interactive_rebase(dir.path()).await.unwrap();

        let repo = crate::repo::open(dir.path()).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap(), original_head);
        assert_eq!(repo.state(), git2::RepositoryState::Clean);
    }

    #[test]
    fn a_plain_rebase_conflict_is_not_marked_as_interactive() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        crate::branch::create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        crate::branch::checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");

        let outcome = crate::branch::rebase_branch(&repo, "main").unwrap();
        assert!(matches!(outcome, RebaseOutcome::Conflicts(_)));

        assert!(!is_interactive_rebase_in_progress(dir.path()).unwrap());
    }
}
