// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Native git worktree management: list, add (new branch or an existing one), and remove.
//! Pure `git2` — `Repository::worktrees`/`find_worktree`/`worktree` cover every operation
//! here directly, no shell-out needed. Detached-HEAD worktrees aren't supported: libgit2's
//! own `git_worktree_add` rejects a non-branch reference outright, so there's no binding for
//! `git worktree add --detach` to call.

use std::path::{Path, PathBuf};

use git2::{BranchType, Repository, WorktreeAddOptions, WorktreeLockStatus, WorktreePruneOptions};
use serde::Serialize;

use crate::branch;
use crate::diff;
use crate::error::{PushGitError, PushGitResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// The `.git/worktrees/<name>` admin identifier for a linked worktree; a fixed sentinel
    /// for the main one (which has no such identifier of its own).
    pub name: String,
    pub path: String,
    /// `None` for a detached HEAD (not reachable via this module's own `add_worktree`, but
    /// possible for a pre-existing worktree PushGit didn't create) or when `is_missing`.
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_missing: bool,
    pub is_dirty: bool,
    pub is_locked: bool,
}

const MAIN_WORKTREE_NAME: &str = "(main)";

/// Lists the main working directory first, then every linked worktree. `git_worktree_list`
/// only ever returns the linked ones (confirmed against libgit2's own doc comment) — matching
/// real `git worktree list`'s "main tree first" framing, the main entry is synthesized here.
/// A linked worktree whose directory or gitdir has been removed by hand (not via
/// `remove_worktree`) is reported with `is_missing: true` and no branch/dirty data rather than
/// failing the whole call — one bad entry shouldn't hide every other, healthy one.
///
/// Works correctly even when `repo` is itself opened from a *linked* worktree (e.g. right
/// after switching into one) — `repo.workdir()` alone can't be trusted to mean "the main
/// worktree" in that case: it correctly reports the linked worktree's own directory, not the
/// main one, and that same directory would then also show up a second time via
/// `repo.worktrees()`, which always enumerates every linked worktree regardless of which one
/// you're standing in, including the current one. Found by hand: creating a worktree and
/// auto-opening it (`add_worktree`'s whole point) immediately reproduced exactly this —
/// the freshly-created worktree listed twice, once mislabeled `is_main`.
pub fn list_worktrees(repo: &Repository) -> PushGitResult<Vec<WorktreeInfo>> {
    let main_path = main_worktree_path(repo)?;
    let (path, branch, is_dirty) = workdir_info(&main_path)?;
    let mut result = vec![WorktreeInfo {
        name: MAIN_WORKTREE_NAME.to_string(),
        path,
        branch,
        is_main: true,
        is_missing: false,
        is_dirty,
        is_locked: false,
    }];

    for name in repo.worktrees()?.iter().flatten() {
        result.push(linked_worktree_info(repo, name)?);
    }

    Ok(result)
}

/// The repository's true main working directory, even when `repo` is itself a linked
/// worktree. `git2` has no binding for libgit2's own `git_repository_commondir()` (unlike
/// `git_repository_path()`/`workdir()`, both bound), so for a linked worktree — `repo.path()`
/// is that worktree's own admin directory, `<main>/.git/worktrees/<name>/` — this reads the
/// `commondir` file libgit2 itself always writes there, pointing back at the shared `.git`
/// directory; the main worktree is that directory's parent. Same "read a stable git-internal
/// file directly since there's no API for it" pattern already used elsewhere in this codebase
/// (e.g. `watcher`'s `MERGE_HEAD` presence check). Doesn't handle a `core.worktree` override
/// repointing the main repo's own workdir away from its `.git`'s parent — a rarer case this
/// isn't chasing.
fn main_worktree_path(repo: &Repository) -> PushGitResult<PathBuf> {
    if !repo.is_worktree() {
        return repo
            .workdir()
            .map(Path::to_path_buf)
            .ok_or_else(|| invalid("repository has no working directory"));
    }

    crate::repo::common_dir(repo)
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid("could not determine the main working directory"))
}

fn linked_worktree_info(repo: &Repository, name: &str) -> PushGitResult<WorktreeInfo> {
    let worktree = repo.find_worktree(name)?;

    if worktree.validate().is_err() {
        return Ok(WorktreeInfo {
            name: name.to_string(),
            // Can't normalize via `workdir_info` below — the directory doesn't exist to open.
            // Never the currently-open repo (you can't have opened a missing directory), so
            // this raw, unnormalized form is never compared against `repoPath` on the
            // frontend anyway.
            path: worktree.path().to_string_lossy().into_owned(),
            branch: None,
            is_main: false,
            is_missing: true,
            is_dirty: false,
            is_locked: false,
        });
    }

    let is_locked = matches!(worktree.is_locked(), Ok(WorktreeLockStatus::Locked(_)));
    let (path, branch, is_dirty) = workdir_info(worktree.path())?;

    Ok(WorktreeInfo {
        name: name.to_string(),
        path,
        branch,
        is_main: false,
        is_missing: false,
        is_dirty,
        is_locked,
    })
}

/// Opens `path` as an ordinary repository and reads its own workdir path (normalized),
/// branch, and dirty state — the same two `diff::` calls the working-directory panel already
/// makes for the currently-open repo. The workdir path is deliberately re-derived via
/// `Repository::workdir()` here rather than reusing whatever `PathBuf` the caller already
/// had, because `git2` isn't consistent about a trailing slash: `workdir()` always includes
/// one, `Worktree::path()` and plain filesystem `Path` manipulation (e.g. deriving the main
/// worktree from `commondir`'s parent) never do. `commands::open_repository` — which is what
/// actually populates the frontend's `repoPath` the "Current" badge compares against — uses
/// this exact `workdir()`-or-`path()` derivation, so every `WorktreeInfo.path` has to match it
/// byte-for-byte or "is this the repo I currently have open" silently never matches.
fn workdir_info(path: &Path) -> PushGitResult<(String, Option<String>, bool)> {
    let repo = Repository::open(path)?;
    let normalized_path = repo
        .workdir()
        .unwrap_or_else(|| repo.path())
        .display()
        .to_string();
    let branch = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(str::to_string));
    let dirty = !diff::diff_unstaged(&repo)?.is_empty() || !diff::diff_staged(&repo)?.is_empty();
    Ok((normalized_path, branch, dirty))
}

/// Adds a new linked worktree at `path`, checked out to `branch_name` — an existing local
/// branch, an existing remote-tracking branch (DWIM'd into a new local tracking branch first,
/// exactly like `checkout_remote_branch`), or, if neither exists, a brand new branch created
/// from `start_point` (`None` = HEAD). `start_point` is ignored once `branch_name` already
/// resolves to something checked-out-able. Whether that branch is already checked out
/// elsewhere (the main worktree or another linked one) is enforced by libgit2 itself
/// (`git_branch_is_checked_out`, inside `Repository::worktree`) — surfaced unchanged rather
/// than re-checked here.
pub fn add_worktree(
    repo: &Repository,
    branch_name: &str,
    start_point: Option<&str>,
    path: &Path,
) -> PushGitResult<()> {
    let admin_name = worktree_admin_name(repo, path)?;
    let branch_ref = resolve_or_create_branch(repo, branch_name, start_point)?;

    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&branch_ref));
    repo.worktree(&admin_name, path, Some(&opts))?;
    Ok(())
}

/// The `.git/worktrees/<name>` identifier, derived from `path`'s own final component rather
/// than the branch name — a branch name may contain `/` (`feature/login`), which can't
/// sensibly be a single directory name under `worktrees/`. Checked against every existing
/// worktree first so a collision surfaces as a clear, friendly error before any directory is
/// touched, rather than the lower-level `EEXISTS`-class error `git_worktree_add`'s own
/// `GIT_MKDIR_EXCL`-flagged directory creation would otherwise produce.
fn worktree_admin_name(repo: &Repository, path: &Path) -> PushGitResult<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| invalid("worktree path must end in a valid directory name"))?
        .to_string();

    if repo
        .worktrees()?
        .iter()
        .flatten()
        .any(|existing| existing == name)
    {
        return Err(invalid(format!(
            "a worktree named '{name}' already exists for this repository"
        )));
    }

    Ok(name)
}

fn resolve_or_create_branch<'repo>(
    repo: &'repo Repository,
    branch_name: &str,
    start_point: Option<&str>,
) -> PushGitResult<git2::Reference<'repo>> {
    if let Ok(existing) = repo.find_branch(branch_name, BranchType::Local) {
        return Ok(existing.into_reference());
    }

    if repo.find_branch(branch_name, BranchType::Remote).is_ok() {
        let local_name = branch::ensure_local_tracking_branch(repo, branch_name)?;
        return Ok(repo
            .find_branch(&local_name, BranchType::Local)?
            .into_reference());
    }

    branch::create_branch(repo, branch_name, start_point)?;
    Ok(repo
        .find_branch(branch_name, BranchType::Local)?
        .into_reference())
}

/// Removes a linked worktree: deletes its on-disk directory and the `.git/worktrees/<name>`
/// admin metadata. Leaves the branch it had checked out intact — no longer checked out
/// anywhere once this returns, but deleting it is a separate, ordinary `branch::delete_branch`
/// call, matching how deleting a remote-tracking branch alongside a local one is already a
/// separate step from deleting the local branch itself. `valid(true)` is required because a
/// healthy worktree isn't prunable by default (that default is what stops `git worktree
/// prune` from deleting a worktree that's merely, say, on an unmounted drive); deliberately
/// does *not* pass `locked(true)` — a locked worktree refuses with libgit2's own clear error
/// instead of being silently overridden, since this module has no unlock action of its own.
pub fn remove_worktree(repo: &Repository, name: &str) -> PushGitResult<()> {
    let worktree = repo.find_worktree(name)?;
    let mut opts = WorktreePruneOptions::new();
    opts.valid(true).working_tree(true);
    worktree.prune(Some(&mut opts))?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> PushGitError {
    PushGitError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use git2::RepositoryInitOptions;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

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

    fn worktree_path(parent: &TempDir, leaf: &str) -> PathBuf {
        parent.path().join(leaf)
    }

    #[test]
    fn lists_the_main_worktree_first_with_no_linked_ones() {
        let (_dir, repo) = repo_init();

        let list = list_worktrees(&repo).unwrap();

        assert_eq!(list.len(), 1);
        assert!(list[0].is_main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert!(!list[0].is_dirty);
    }

    #[test]
    fn add_worktree_with_a_new_branch_creates_branch_and_checks_it_out() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "tracked.txt", "content\n");
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");

        add_worktree(&repo, "feature", None, &path).unwrap();

        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
        let wt_repo = Repository::open(&path).unwrap();
        assert_eq!(wt_repo.head().unwrap().shorthand(), Some("feature"));
        assert_eq!(
            fs::read_to_string(path.join("tracked.txt")).unwrap(),
            "content\n",
            "the branch's tree should actually be checked out onto disk, not just linked"
        );
    }

    #[test]
    fn add_worktree_with_a_new_branch_from_an_explicit_start_point() {
        let (_dir, repo) = repo_init();
        let base_oid = repo.head().unwrap().target().unwrap();
        commit_file(&repo, "a.txt", "a\n");
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "from-base");

        add_worktree(&repo, "from-base", Some(&base_oid.to_string()), &path).unwrap();

        let wt_repo = Repository::open(&path).unwrap();
        assert_eq!(
            wt_repo.head().unwrap().peel_to_commit().unwrap().id(),
            base_oid
        );
        assert!(!path.join("a.txt").exists());
    }

    #[test]
    fn add_worktree_with_an_existing_local_branch_ignores_start_point() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "main-only.txt", "x\n");
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");

        add_worktree(&repo, "feature", Some("main"), &path).unwrap();

        let wt_repo = Repository::open(&path).unwrap();
        assert_eq!(wt_repo.head().unwrap().shorthand(), Some("feature"));
        assert!(!path.join("main-only.txt").exists());
    }

    #[test]
    fn add_worktree_with_a_remote_only_branch_creates_a_local_tracking_branch() {
        let (dir, repo) = repo_init();
        let remote_path = dir.path().to_str().unwrap();
        repo.remote("origin", remote_path).unwrap();
        let oid = repo.head().unwrap().target().unwrap();
        repo.reference("refs/remotes/origin/feature", oid, true, "")
            .unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");

        add_worktree(&repo, "origin/feature", None, &path).unwrap();

        let branch = repo.find_branch("feature", BranchType::Local).unwrap();
        assert_eq!(
            branch.upstream().unwrap().name().unwrap(),
            Some("origin/feature")
        );
        let wt_repo = Repository::open(&path).unwrap();
        assert_eq!(wt_repo.head().unwrap().shorthand(), Some("feature"));
    }

    #[test]
    fn add_worktree_refuses_a_branch_already_checked_out_in_the_main_worktree() {
        let (_dir, repo) = repo_init();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "main-wt");

        let result = add_worktree(&repo, "main", None, &path);

        assert!(result.is_err());
    }

    #[test]
    fn add_worktree_refuses_a_branch_already_checked_out_in_another_linked_worktree() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        add_worktree(&repo, "feature", None, &worktree_path(&parent, "first")).unwrap();

        let result = add_worktree(&repo, "feature", None, &worktree_path(&parent, "second"));

        assert!(result.is_err());
    }

    #[test]
    fn add_worktree_refuses_a_colliding_admin_name_before_touching_disk() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "a", None).unwrap();
        branch::create_branch(&repo, "b", None).unwrap();
        let parent = TempDir::new().unwrap();
        add_worktree(&repo, "a", None, &worktree_path(&parent, "shared-name")).unwrap();

        let other_parent = TempDir::new().unwrap();
        let result = add_worktree(
            &repo,
            "b",
            None,
            &worktree_path(&other_parent, "shared-name"),
        );

        assert!(result.is_err());
        assert!(!worktree_path(&other_parent, "shared-name").exists());
    }

    #[test]
    fn list_worktrees_reports_a_linked_worktree_and_its_dirty_state() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();

        let clean_list = list_worktrees(&repo).unwrap();
        let linked = clean_list.iter().find(|w| !w.is_main).unwrap();
        assert_eq!(linked.branch.as_deref(), Some("feature"));
        assert!(!linked.is_dirty);
        assert!(!linked.is_missing);

        fs::write(path.join("untracked.txt"), "x\n").unwrap();
        let dirty_list = list_worktrees(&repo).unwrap();
        assert!(dirty_list.iter().find(|w| !w.is_main).unwrap().is_dirty);
    }

    #[test]
    fn list_worktrees_marks_an_externally_deleted_worktree_as_missing_without_failing() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();

        fs::remove_dir_all(&path).unwrap();

        let list = list_worktrees(&repo).unwrap();
        let linked = list.iter().find(|w| !w.is_main).unwrap();
        assert!(linked.is_missing);
        assert_eq!(linked.branch, None);
    }

    /// Regression test: right after `add_worktree` + switching into the new worktree (the
    /// exact "auto-open on create" flow this feature exists for), listing from *that*
    /// worktree's own repo handle used to report its own directory as `is_main` (since
    /// `repo.workdir()` is never wrong about "my own directory," just wrong about whether
    /// that directory is *the* main one) and then list it a second time via
    /// `repo.worktrees()`, which enumerates every linked worktree unconditionally, including
    /// whichever one you're standing in.
    #[test]
    fn list_worktrees_from_inside_a_linked_worktree_reports_the_real_main_path_once() {
        let (dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();

        let repo_from_worktree = Repository::open(&path).unwrap();
        assert!(repo_from_worktree.is_worktree());

        let list = list_worktrees(&repo_from_worktree).unwrap();

        let main_entries: Vec<_> = list.iter().filter(|w| w.is_main).collect();
        assert_eq!(main_entries.len(), 1, "exactly one entry should be is_main");
        let expected_main_path = Repository::open(dir.path())
            .unwrap()
            .workdir()
            .unwrap()
            .display()
            .to_string();
        assert_eq!(main_entries[0].path, expected_main_path);
        assert_eq!(main_entries[0].branch.as_deref(), Some("main"));

        let linked_entries: Vec<_> = list.iter().filter(|w| !w.is_main).collect();
        assert_eq!(
            linked_entries.len(),
            1,
            "the worktree itself, not duplicated"
        );
        assert_eq!(linked_entries[0].name, "feature-wt");
    }

    #[test]
    fn remove_worktree_deletes_directory_and_frees_the_branch() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();

        remove_worktree(&repo, "feature-wt").unwrap();

        assert!(!path.exists());
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
        // The branch is no longer checked out anywhere, so it can now host another worktree.
        let other_parent = TempDir::new().unwrap();
        add_worktree(
            &repo,
            "feature",
            None,
            &worktree_path(&other_parent, "again"),
        )
        .unwrap();
    }

    #[test]
    fn remove_worktree_succeeds_even_with_uncommitted_changes() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();
        fs::write(path.join("untracked.txt"), "x\n").unwrap();

        remove_worktree(&repo, "feature-wt").unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn workdir_info_opens_a_path_as_an_ordinary_repository() {
        let mut opts = RepositoryInitOptions::new();
        opts.initial_head("main");
        let td = TempDir::new().unwrap();
        let repo = Repository::init_opts(td.path(), &opts).unwrap();
        crate::test_support::set_test_identity(&repo);
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();

        let (path, branch, dirty) = workdir_info(td.path()).unwrap();

        assert_eq!(branch.as_deref(), Some("main"));
        assert!(!dirty);
        assert!(
            path.ends_with('/'),
            "must match Repository::workdir()'s own trailing-slash convention, \
             the same one commands::open_repository uses for repoPath: got {path:?}"
        );
    }

    /// Regression test for the trailing-slash mismatch found by hand: `Worktree::path()`
    /// never has one, `Repository::workdir()` (what `commands::open_repository` actually
    /// returns as `repoPath`) always does — so before `workdir_info` normalized it, a
    /// worktree that genuinely was the currently-open repo could never be recognized as
    /// `entry.path === repoPath` on the frontend.
    #[test]
    fn linked_worktree_info_normalizes_its_path_to_match_workdir_convention() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "feature", None).unwrap();
        let parent = TempDir::new().unwrap();
        let path = worktree_path(&parent, "feature-wt");
        add_worktree(&repo, "feature", None, &path).unwrap();

        let list = list_worktrees(&repo).unwrap();
        let linked = list.iter().find(|w| !w.is_main).unwrap();

        assert!(linked.path.ends_with('/'));
        let expected = Repository::open(&path)
            .unwrap()
            .workdir()
            .unwrap()
            .display()
            .to_string();
        assert_eq!(linked.path, expected);
    }
}
