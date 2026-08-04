// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Branch create/checkout/merge/delete, tag management, local merge-conflict state
//! tracking.
//!
//! Non-interactive rebase is implemented here via git2's own `Rebase` API (the
//! cherry-pick-based `rebase-merge` machinery libgit2 supports natively) — only
//! *interactive* rebase (reorder/squash/reword) is a hard
//! gap requiring the real `git` binary, and that stays out of MVP scope entirely.

mod model;

pub use model::{BranchInfo, CherryPickOutcome, MergeOutcome, RebaseOutcome, RepoState, ResetMode};

use git2::{BranchType, Index, Repository, ResetType};

use crate::diff::{self, ConflictSides};
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

pub fn list_conflicts(repo: &Repository) -> PushGitResult<Vec<String>> {
    collect_conflict_paths(&repo.index()?)
}

/// Marks a conflicted path resolved by staging its current working-tree content — the
/// caller (frontend, driven by the 3-way conflict editor) is responsible for having
/// already written the resolved content to disk.
pub fn resolve_conflict(repo: &Repository, path: &str) -> PushGitResult<()> {
    let mut index = repo.index()?;
    index.add_path(std::path::Path::new(path))?;
    index.write()?;
    Ok(())
}

/// The base/ours/theirs content of one unresolved conflict, plus the ours-vs-theirs diff
/// driving the 3-way conflict editor's per-hunk resolution controls.
pub fn conflict_sides(repo: &Repository, path: &str) -> PushGitResult<ConflictSides> {
    let index = repo.index()?;
    let mut found = None;
    for conflict in index.conflicts()? {
        let conflict = conflict?;
        let entry = conflict
            .our
            .as_ref()
            .or(conflict.their.as_ref())
            .or(conflict.ancestor.as_ref());
        if entry.map(|e| e.path == path.as_bytes()).unwrap_or(false) {
            found = Some(conflict);
            break;
        }
    }
    let conflict = found.ok_or_else(|| invalid(format!("no conflict at path '{path}'")))?;

    let base_blob = conflict
        .ancestor
        .as_ref()
        .map(|e| repo.find_blob(e.id))
        .transpose()?;
    let ours_blob = conflict
        .our
        .as_ref()
        .map(|e| repo.find_blob(e.id))
        .transpose()?;
    let theirs_blob = conflict
        .their
        .as_ref()
        .map(|e| repo.find_blob(e.id))
        .transpose()?;

    let (hunks, is_binary) =
        diff::diff_blob_content(repo, ours_blob.as_ref(), theirs_blob.as_ref())?;

    Ok(ConflictSides {
        base: base_blob.map(|b| String::from_utf8_lossy(b.content()).into_owned()),
        ours: ours_blob.map(|b| String::from_utf8_lossy(b.content()).into_owned()),
        theirs: theirs_blob.map(|b| String::from_utf8_lossy(b.content()).into_owned()),
        is_binary,
        hunks,
    })
}

/// Writes the conflict editor's resolved content to the working tree and stages it in one
/// step — the counterpart to `resolve_conflict` for callers that have the resolved text in
/// memory (the 3-way editor) rather than already on disk (the "edit externally" workaround).
pub fn write_resolved_conflict(repo: &Repository, path: &str, content: &str) -> PushGitResult<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| invalid("repository has no working directory"))?;
    std::fs::write(workdir.join(path), content)?;
    resolve_conflict(repo, path)
}

/// Resolves a delete/modify conflict (`ConflictSides::ours` or `::theirs` is `None`) by
/// deleting the path — the counterpart to `write_resolved_conflict` for when the chosen
/// resolution is "no file" rather than some text content.
pub fn resolve_conflict_as_deleted(repo: &Repository, path: &str) -> PushGitResult<()> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| invalid("repository has no working directory"))?;
    let full_path = workdir.join(path);
    if full_path.exists() {
        std::fs::remove_file(full_path)?;
    }
    let mut index = repo.index()?;
    index.remove_path(std::path::Path::new(path))?;
    index.write()?;
    Ok(())
}

pub fn list_tags(repo: &Repository) -> PushGitResult<Vec<String>> {
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = std::str::from_utf8(name)
            .ok()
            .and_then(|n| n.strip_prefix("refs/tags/"))
        {
            tags.push(short.to_string());
        }
        true
    })?;
    Ok(tags)
}

/// Creates a tag at `at` (or HEAD if not given). An annotated tag is created when
/// `message` is provided; otherwise a lightweight tag.
pub fn create_tag(
    repo: &Repository,
    name: &str,
    at: Option<&str>,
    message: Option<&str>,
) -> PushGitResult<()> {
    let target = match at {
        Some(rev) => repo.revparse_single(rev)?,
        None => repo.head()?.peel(git2::ObjectType::Commit)?,
    };

    match message {
        Some(message) => {
            let signature = repo.signature()?;
            repo.tag(name, &target, &signature, message, false)?;
        }
        None => {
            repo.tag_lightweight(name, &target, false)?;
        }
    }

    Ok(())
}

pub fn delete_tag(repo: &Repository, name: &str) -> PushGitResult<()> {
    repo.tag_delete(name)?;
    Ok(())
}

/// Force-moves an existing tag to `to` (any commit-ish) — the graph's drag-a-tag-onto-a-
/// commit-or-branch-badge gesture. Preserves whichever kind the tag already was: an
/// annotated tag is re-created at the new target with its original message (a tag object is
/// immutable and encodes its target, so "moving" one really means creating a new tag object
/// and force-updating the ref to point at it); a lightweight tag is just re-pointed.
pub fn move_tag(repo: &Repository, name: &str, to: &str) -> PushGitResult<()> {
    let reference = repo.find_reference(&format!("refs/tags/{name}"))?;
    let immediate_oid = reference
        .target()
        .ok_or_else(|| invalid(format!("tag '{name}' has no direct target")))?;
    let target = repo.revparse_single(to)?;

    match repo.find_tag(immediate_oid) {
        Ok(existing) => {
            let message = existing.message().unwrap_or_default().to_string();
            let signature = repo.signature()?;
            repo.tag(name, &target, &signature, &message, true)?;
        }
        Err(_) => {
            repo.tag_lightweight(name, &target, true)?;
        }
    }

    Ok(())
}

/// Renames a tag, preserving its target and (for an annotated tag) its message — git has no
/// native tag-rename, so this creates `new_name` at `old_name`'s current target and then
/// deletes `old_name`. Creates before deleting so a name collision on `new_name` leaves
/// `old_name` intact rather than losing the tag.
pub fn rename_tag(repo: &Repository, old_name: &str, new_name: &str) -> PushGitResult<()> {
    let reference = repo.find_reference(&format!("refs/tags/{old_name}"))?;
    let immediate_oid = reference
        .target()
        .ok_or_else(|| invalid(format!("tag '{old_name}' has no direct target")))?;

    match repo.find_tag(immediate_oid) {
        Ok(existing) => {
            let message = existing.message().unwrap_or_default().to_string();
            let signature = repo.signature()?;
            let target = existing.target()?;
            repo.tag(new_name, &target, &signature, &message, false)?;
        }
        Err(_) => {
            let target = repo.find_object(immediate_oid, None)?;
            repo.tag_lightweight(new_name, &target, false)?;
        }
    }

    repo.tag_delete(old_name)?;
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

    #[test]
    fn resolve_conflict_stages_the_working_tree_content() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();

        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        resolve_conflict(&repo, "shared.txt").unwrap();

        assert!(list_conflicts(&repo).unwrap().is_empty());
    }

    /// Sets up the same diverging-branches merge conflict as
    /// `resolve_conflict_stages_the_working_tree_content`, leaving `repo` mid-merge with
    /// exactly one conflicted path, "shared.txt".
    fn conflicted_repo() -> (tempfile::TempDir, Repository) {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();
        (dir, repo)
    }

    #[test]
    fn conflict_sides_reports_base_ours_and_theirs_content() {
        let (_dir, repo) = conflicted_repo();

        let sides = conflict_sides(&repo, "shared.txt").unwrap();

        assert_eq!(sides.base.as_deref(), Some("base\n"));
        assert_eq!(sides.ours.as_deref(), Some("main version\n"));
        assert_eq!(sides.theirs.as_deref(), Some("feature version\n"));
        assert!(!sides.is_binary);
        assert!(!sides.hunks.is_empty());
    }

    #[test]
    fn conflict_sides_errors_for_a_path_with_no_conflict() {
        let (_dir, repo) = conflicted_repo();

        assert!(conflict_sides(&repo, "no-such-file.txt").is_err());
    }

    #[test]
    fn write_resolved_conflict_writes_the_file_and_stages_it() {
        let (dir, repo) = conflicted_repo();

        write_resolved_conflict(&repo, "shared.txt", "resolved by hand\n").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "resolved by hand\n"
        );
        assert!(list_conflicts(&repo).unwrap().is_empty());
    }

    fn commit_delete_file(repo: &Repository, name: &str) -> git2::Oid {
        fs::remove_file(repo.workdir().unwrap().join(name)).unwrap();
        let mut index = repo.index().unwrap();
        index.remove_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &format!("delete {name}"),
            &tree,
            &[&head],
        )
        .unwrap()
    }

    #[test]
    fn conflict_sides_reports_a_missing_side_as_none_for_a_delete_modify_conflict() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_delete_file(&repo, "shared.txt");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();

        let sides = conflict_sides(&repo, "shared.txt").unwrap();

        assert_eq!(sides.base.as_deref(), Some("base\n"));
        assert_eq!(sides.ours, None);
        assert_eq!(sides.theirs.as_deref(), Some("feature version\n"));
    }

    #[test]
    fn resolve_conflict_as_deleted_removes_the_path_from_disk_and_the_index() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_delete_file(&repo, "shared.txt");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();

        resolve_conflict_as_deleted(&repo, "shared.txt").unwrap();

        assert!(!dir.path().join("shared.txt").exists());
        assert!(list_conflicts(&repo).unwrap().is_empty());
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
        assert_eq!(repo_state(&repo), RepoState::Clean);
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
        assert_eq!(repo_state(&repo), RepoState::Clean);

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

    #[test]
    fn repo_state_is_clean_by_default() {
        let (_dir, repo) = repo_init();
        assert_eq!(repo_state(&repo), RepoState::Clean);
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
        assert_eq!(repo_state(&repo), RepoState::Clean);
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
        assert_eq!(repo_state(&repo), RepoState::CherryPick);

        abort_cherry_pick(&repo).unwrap();

        assert_eq!(repo_state(&repo), RepoState::Clean);
        assert_eq!(repo.head().unwrap().target().unwrap(), main_tip);
        assert_eq!(
            fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
            "main version\n"
        );
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

    #[test]
    fn create_list_and_delete_lightweight_and_annotated_tags() {
        let (_dir, repo) = repo_init();

        create_tag(&repo, "v1.0.0", None, None).unwrap();
        create_tag(&repo, "v1.1.0", None, Some("release notes")).unwrap();

        let mut tags = list_tags(&repo).unwrap();
        tags.sort();
        assert_eq!(tags, vec!["v1.0.0".to_string(), "v1.1.0".to_string()]);

        delete_tag(&repo, "v1.0.0").unwrap();
        assert_eq!(list_tags(&repo).unwrap(), vec!["v1.1.0".to_string()]);
    }

    #[test]
    fn move_tag_repoints_a_lightweight_tag() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(&repo, "v1.0.0", Some(&first.to_string()), None).unwrap();

        move_tag(&repo, "v1.0.0", &second.to_string()).unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        assert_eq!(reference.target(), Some(second));
        // Still lightweight — moving it shouldn't turn it into an annotated tag.
        assert!(repo.find_tag(reference.target().unwrap()).is_err());
    }

    #[test]
    fn move_tag_repoints_an_annotated_tag_and_keeps_its_message() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(
            &repo,
            "v1.0.0",
            Some(&first.to_string()),
            Some("release notes"),
        )
        .unwrap();

        move_tag(&repo, "v1.0.0", &second.to_string()).unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        let tag_object = repo.find_tag(reference.target().unwrap()).unwrap();
        assert_eq!(tag_object.target_id(), second);
        assert_eq!(tag_object.message(), Some("release notes"));
    }

    #[test]
    fn move_tag_accepts_a_branch_name_as_the_target() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(&repo, "v1.0.0", Some(&first.to_string()), None).unwrap();

        move_tag(&repo, "v1.0.0", "main").unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        assert_eq!(reference.target(), Some(second));
    }

    #[test]
    fn rename_tag_renames_a_lightweight_tag_and_keeps_its_target() {
        let (_dir, repo) = repo_init();
        let commit = repo.head().unwrap().peel_to_commit().unwrap().id();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        rename_tag(&repo, "v1.0.0", "v1.0.0-renamed").unwrap();

        assert!(repo.find_reference("refs/tags/v1.0.0").is_err());
        let reference = repo.find_reference("refs/tags/v1.0.0-renamed").unwrap();
        assert_eq!(reference.target(), Some(commit));
        assert!(repo.find_tag(reference.target().unwrap()).is_err());
    }

    #[test]
    fn rename_tag_renames_an_annotated_tag_and_keeps_its_message() {
        let (_dir, repo) = repo_init();
        let commit = repo.head().unwrap().peel_to_commit().unwrap().id();
        create_tag(&repo, "v1.0.0", None, Some("release notes")).unwrap();

        rename_tag(&repo, "v1.0.0", "v1.0.0-renamed").unwrap();

        assert!(repo.find_reference("refs/tags/v1.0.0").is_err());
        let reference = repo.find_reference("refs/tags/v1.0.0-renamed").unwrap();
        let tag_object = repo.find_tag(reference.target().unwrap()).unwrap();
        assert_eq!(tag_object.target_id(), commit);
        assert_eq!(tag_object.message(), Some("release notes"));
    }

    #[test]
    fn rename_tag_leaves_the_old_tag_intact_when_the_new_name_is_taken() {
        let (_dir, repo) = repo_init();
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        create_tag(&repo, "v2.0.0", None, None).unwrap();

        assert!(rename_tag(&repo, "v1.0.0", "v2.0.0").is_err());

        assert!(repo.find_reference("refs/tags/v1.0.0").is_ok());
        assert!(repo.find_reference("refs/tags/v2.0.0").is_ok());
    }
}
