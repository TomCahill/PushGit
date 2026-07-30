// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! App-level undo/redo operation stack — distinct from git's own
//! reflog. Replaces the single-slot `branch::snapshot_head`/`undo_to_snapshot` mechanism.
//!
//! Only "destructive/hard-to-reverse" operations push an entry: commit/amend, branch
//! checkout/delete/rename, merge, rebase (+ continue), pull, stash apply/pop/drop, tag
//! delete, conflict resolution, and discarding a file's changes. Trivially-reversible
//! actions (stage/unstage — one more click undoes them) are deliberately excluded to keep
//! the history usable rather than one entry per click.
//!
//! Each entry is a full snapshot of everything a wrapped operation could plausibly touch —
//! HEAD, every local branch tip, every tag, the stash list, the index tree, and the working
//! directory tree — captured as content-addressed git objects (cheap: no file copying).
//! Capturing the *whole* ref set rather than just HEAD is what makes undo work for
//! operations like branch delete/rename that never move HEAD at all. Restoring an entry
//! reconciles every one of those categories back to the captured state, deleting anything
//! that didn't exist yet and creating/repointing anything that did.
//!
//! **Reachability**: a snapshot's captured oids (branch tips, tag targets, stash commits,
//! the index/workdir trees) are kept alive against `git gc`/`git maintenance` by one
//! throwaway "manifest" commit per stack slot,
//! parented on every commit the snapshot needs to survive, anchored by a ref under
//! `refs/pushgit/undo/<slot>`. The ref (and its manifest commit) is deleted once the entry is
//! evicted from the stack (by depth or by a new action clearing the redo branch).

mod stack;

pub use stack::{OperationSummary, UndoLog, UndoRedoStatus};

use std::collections::{BTreeMap, HashSet};

use git2::{BranchType, Oid, Repository};

use crate::error::{PushGitError, PushGitResult};

/// One point-in-time capture of everything a wrapped operation could touch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    head: HeadState,
    branches: BTreeMap<String, Oid>,
    tags: Vec<TagSnapshot>,
    stash: Vec<StashSnapshot>,
    index_tree: Oid,
    workdir_tree: Oid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HeadState {
    /// HEAD is a symbolic ref to a branch that has at least one commit.
    Branch(String),
    /// HEAD is a symbolic ref to a branch with no commits yet.
    Unborn(String),
    /// HEAD points directly at a commit.
    Detached(Oid),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TagSnapshot {
    name: String,
    target_commit: Oid,
    /// `None` for a lightweight tag, `Some(message)` for an annotated one. Restoring
    /// re-creates an equivalent tag object rather than the exact original oid (mirrors
    /// `branch::move_tag`'s existing approach) — a tag object is immutable, so this is
    /// indistinguishable from the original in every way that matters.
    message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StashSnapshot {
    oid: Oid,
    message: String,
}

/// Captures the current state of everything a wrapped operation could touch. Pure and
/// read-only — call this *before* running the operation.
pub fn capture(repo: &Repository) -> PushGitResult<Snapshot> {
    Ok(Snapshot {
        head: capture_head(repo)?,
        branches: capture_branches(repo)?,
        tags: capture_tags(repo)?,
        stash: capture_stash(repo)?,
        index_tree: capture_index_tree(repo)?,
        workdir_tree: capture_workdir_tree(repo)?,
    })
}

/// Restores every category captured in `snapshot`, in an order safe for the current repo
/// state: branches/tags/stash first (so HEAD always has somewhere valid to point once it's
/// restored), then HEAD itself, then the working directory and index last (checking out a
/// tree doesn't depend on any ref state above).
pub fn restore(repo: &mut Repository, snapshot: &Snapshot) -> PushGitResult<()> {
    restore_branches(repo, &snapshot.branches)?;
    restore_tags(repo, &snapshot.tags)?;
    restore_stash(repo, &snapshot.stash)?;
    restore_head(repo, &snapshot.head)?;
    restore_workdir_and_index(repo, snapshot)?;
    Ok(())
}

/// Every commit oid a snapshot needs to survive `git gc` until it's either restored to or
/// evicted from the stack — see the module doc's "Reachability" note.
fn keep_alive_oids(snapshot: &Snapshot) -> Vec<Oid> {
    let mut oids: Vec<Oid> = snapshot.branches.values().copied().collect();
    oids.extend(snapshot.tags.iter().map(|t| t.target_commit));
    oids.extend(snapshot.stash.iter().map(|s| s.oid));
    if let HeadState::Detached(oid) = snapshot.head {
        oids.push(oid);
    }
    oids
}

fn capture_head(repo: &Repository) -> PushGitResult<HeadState> {
    match repo.head() {
        Ok(head) => {
            let name = head
                .name()
                .ok_or_else(|| invalid("HEAD reference has no name"))?
                .to_string();
            if head.is_branch() {
                Ok(HeadState::Branch(name))
            } else {
                let oid = head
                    .target()
                    .ok_or_else(|| invalid("detached HEAD has no target"))?;
                Ok(HeadState::Detached(oid))
            }
        }
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
            let head_ref = repo.find_reference("HEAD")?;
            let symbolic = head_ref
                .symbolic_target()
                .ok_or_else(|| invalid("HEAD is not a symbolic reference"))?
                .to_string();
            Ok(HeadState::Unborn(symbolic))
        }
        Err(e) => Err(e.into()),
    }
}

fn capture_branches(repo: &Repository) -> PushGitResult<BTreeMap<String, Oid>> {
    let mut branches = BTreeMap::new();
    for branch in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = branch?;
        let Some(name) = branch.name()? else { continue };
        let Some(oid) = branch.get().target() else {
            continue;
        };
        branches.insert(format!("refs/heads/{name}"), oid);
    }
    Ok(branches)
}

fn capture_tags(repo: &Repository) -> PushGitResult<Vec<TagSnapshot>> {
    let mut names = Vec::new();
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = std::str::from_utf8(name)
            .ok()
            .and_then(|n| n.strip_prefix("refs/tags/"))
        {
            names.push(short.to_string());
        }
        true
    })?;

    let mut tags = Vec::with_capacity(names.len());
    for name in names {
        let reference = repo.find_reference(&format!("refs/tags/{name}"))?;
        let immediate_oid = reference
            .target()
            .ok_or_else(|| invalid(format!("tag '{name}' has no direct target")))?;
        let (target_commit, message) = match repo.find_tag(immediate_oid) {
            Ok(tag_obj) => (
                tag_obj.target()?.peel_to_commit()?.id(),
                Some(tag_obj.message().unwrap_or_default().to_string()),
            ),
            Err(_) => {
                let commit = repo.find_object(immediate_oid, None)?.peel_to_commit()?;
                (commit.id(), None)
            }
        };
        tags.push(TagSnapshot {
            name,
            target_commit,
            message,
        });
    }
    Ok(tags)
}

fn capture_stash(repo: &Repository) -> PushGitResult<Vec<StashSnapshot>> {
    // `stash_foreach` requires `&mut Repository` in git2, but only to guard against
    // concurrent stash mutation during the walk — reading the reflog itself doesn't need
    // exclusive access, so a raw reflog read avoids forcing this whole call chain to `&mut`.
    let mut entries = Vec::new();
    if let Ok(reflog) = repo.reflog("refs/stash") {
        for i in 0..reflog.len() {
            let Some(entry) = reflog.get(i) else { continue };
            entries.push(StashSnapshot {
                oid: entry.id_new(),
                message: entry.message().unwrap_or_default().to_string(),
            });
        }
        // Reflog order is oldest-first; stash order is newest-first (index 0 = top).
        entries.reverse();
    }
    Ok(entries)
}

fn capture_index_tree(repo: &Repository) -> PushGitResult<Oid> {
    let mut index = repo.index()?;
    Ok(index.write_tree()?)
}

/// Captures "as if every workdir change were staged" by momentarily staging everything into
/// the *real* index, writing that as a tree, then restoring the real index to exactly what
/// it was beforehand. There's no owner-less way to scan a working directory with git2 (an
/// in-memory `Index::new()` has no repository to resolve the workdir root against), so this
/// mutate-then-restore sequence — mirroring what `git stash create` does internally — is the
/// standard technique for a non-destructive workdir capture.
fn capture_workdir_tree(repo: &Repository) -> PushGitResult<Oid> {
    let mut index = repo.index()?;
    let original_tree_oid = index.write_tree()?;

    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let workdir_tree = index.write_tree()?;

    let original_tree = repo.find_tree(original_tree_oid)?;
    index.read_tree(&original_tree)?;
    index.write()?;

    Ok(workdir_tree)
}

fn restore_head(repo: &Repository, head: &HeadState) -> PushGitResult<()> {
    match head {
        HeadState::Branch(refname) | HeadState::Unborn(refname) => {
            repo.set_head(refname)?;
        }
        HeadState::Detached(oid) => {
            repo.set_head_detached(*oid)?;
        }
    }
    Ok(())
}

fn restore_branches(repo: &Repository, branches: &BTreeMap<String, Oid>) -> PushGitResult<()> {
    let current = capture_branches(repo)?;

    for name in current.keys() {
        if !branches.contains_key(name) {
            repo.find_reference(name)?.delete()?;
        }
    }
    for (name, oid) in branches {
        if current.get(name) != Some(oid) {
            repo.reference(name, *oid, true, "pushgit: undo/redo")?;
        }
    }
    Ok(())
}

fn restore_tags(repo: &Repository, tags: &[TagSnapshot]) -> PushGitResult<()> {
    let mut current_names = Vec::new();
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = std::str::from_utf8(name)
            .ok()
            .and_then(|n| n.strip_prefix("refs/tags/"))
        {
            current_names.push(short.to_string());
        }
        true
    })?;

    let snapshot_names: HashSet<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    for name in &current_names {
        if !snapshot_names.contains(name.as_str()) {
            repo.tag_delete(name)?;
        }
    }

    for tag in tags {
        let target = repo.find_commit(tag.target_commit)?;
        match &tag.message {
            Some(message) => {
                let signature = repo.signature()?;
                repo.tag(&tag.name, target.as_object(), &signature, message, true)?;
            }
            None => {
                repo.tag_lightweight(&tag.name, target.as_object(), true)?;
            }
        }
    }
    Ok(())
}

fn restore_stash(repo: &mut Repository, stash: &[StashSnapshot]) -> PushGitResult<()> {
    if stash.is_empty() {
        if let Ok(mut reference) = repo.find_reference("refs/stash") {
            reference.delete()?;
        }
        return Ok(());
    }

    repo.reference("refs/stash", stash[0].oid, true, "pushgit: undo/redo")?;

    let mut reflog = repo.reflog("refs/stash")?;
    while !reflog.is_empty() {
        reflog.remove(0, false)?;
    }
    let signature = repo.signature()?;
    // Reflog order is oldest-first; `stash` is newest-first (index 0 = top).
    for entry in stash.iter().rev() {
        reflog.append(entry.oid, &signature, Some(&entry.message))?;
    }
    reflog.write()?;
    Ok(())
}

fn restore_workdir_and_index(repo: &Repository, snapshot: &Snapshot) -> PushGitResult<()> {
    let workdir_tree = repo.find_tree(snapshot.workdir_tree)?;
    let mut checkout = git2::build::CheckoutBuilder::new();
    checkout.force().remove_untracked(true).update_index(false);
    repo.checkout_tree(workdir_tree.as_object(), Some(&mut checkout))?;

    let index_tree = repo.find_tree(snapshot.index_tree)?;
    let mut index = repo.index()?;
    index.read_tree(&index_tree)?;
    index.write()?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> PushGitError {
    PushGitError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch, create_tag, delete_branch, delete_tag};
    use crate::stage::{commit as stage_commit, stage_file};
    use crate::stash::create_stash;
    use crate::test_support::repo_init;
    use std::fs;

    fn commit_file(repo: &Repository, name: &str, content: &str) -> Oid {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        stage_file(repo, name).unwrap();
        stage_commit(repo, name, false, false).unwrap()
    }

    #[test]
    fn undoes_a_commit_back_to_the_prior_head() {
        let (_dir, mut repo) = repo_init();
        let before = repo.head().unwrap().target().unwrap();

        let snapshot = capture(&repo).unwrap();
        commit_file(&repo, "a.txt", "a\n");
        assert_ne!(repo.head().unwrap().target().unwrap(), before);

        restore(&mut repo, &snapshot).unwrap();
        assert_eq!(repo.head().unwrap().target().unwrap(), before);
    }

    #[test]
    fn undoes_a_branch_delete_even_though_head_never_moved() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        let snapshot = capture(&repo).unwrap();
        delete_branch(&repo, "feature").unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_err());

        restore(&mut repo, &snapshot).unwrap();
        assert!(repo.find_branch("feature", BranchType::Local).is_ok());
    }

    #[test]
    fn undoes_a_branch_rename() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "old-name", None).unwrap();

        let snapshot = capture(&repo).unwrap();
        let mut branch = repo.find_branch("old-name", BranchType::Local).unwrap();
        branch.rename("new-name", false).unwrap();
        drop(branch);

        restore(&mut repo, &snapshot).unwrap();
        assert!(repo.find_branch("old-name", BranchType::Local).is_ok());
        assert!(repo.find_branch("new-name", BranchType::Local).is_err());
    }

    #[test]
    fn undoes_a_checkout_back_to_the_previous_branch() {
        let (_dir, mut repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();

        let snapshot = capture(&repo).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("feature"));

        restore(&mut repo, &snapshot).unwrap();
        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
    }

    #[test]
    fn undoes_a_tag_delete_preserving_an_annotated_message() {
        let (_dir, mut repo) = repo_init();
        create_tag(&repo, "v1.0.0", None, Some("release notes")).unwrap();

        let snapshot = capture(&repo).unwrap();
        delete_tag(&repo, "v1.0.0").unwrap();
        assert!(repo.find_reference("refs/tags/v1.0.0").is_err());

        restore(&mut repo, &snapshot).unwrap();
        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        let tag_obj = repo.find_tag(reference.target().unwrap()).unwrap();
        assert_eq!(tag_obj.message(), Some("release notes"));
    }

    #[test]
    fn undoes_a_lightweight_tag_delete() {
        let (_dir, mut repo) = repo_init();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        let snapshot = capture(&repo).unwrap();
        delete_tag(&repo, "v1.0.0").unwrap();

        restore(&mut repo, &snapshot).unwrap();
        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        assert!(repo.find_tag(reference.target().unwrap()).is_err());
    }

    #[test]
    fn undoes_a_stash_drop() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        create_stash(&mut repo, Some("wip")).unwrap();

        let snapshot = capture(&repo).unwrap();
        repo.stash_drop(0).unwrap();
        assert!(repo.find_reference("refs/stash").is_err());

        restore(&mut repo, &snapshot).unwrap();
        let mut seen = Vec::new();
        repo.stash_foreach(|_, message, _| {
            seen.push(message.to_string());
            true
        })
        .unwrap();
        assert_eq!(seen, vec!["On main: wip".to_string()]);
    }

    #[test]
    fn undoes_a_stash_pop_restoring_both_the_stash_entry_and_the_workdir() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        create_stash(&mut repo, None).unwrap();

        let snapshot = capture(&repo).unwrap();
        repo.stash_pop(0, None).unwrap();
        assert!(dir.path().join("dirty.txt").exists());
        assert!(repo.find_reference("refs/stash").is_err());

        restore(&mut repo, &snapshot).unwrap();
        assert!(
            !dir.path().join("dirty.txt").exists(),
            "workdir should be back to clean, pre-pop state"
        );
        let mut count = 0;
        repo.stash_foreach(|_, _, _| {
            count += 1;
            true
        })
        .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn restores_unstaged_and_staged_changes_independently() {
        let (dir, mut repo) = repo_init();
        commit_file(&repo, "tracked.txt", "original\n");
        fs::write(dir.path().join("tracked.txt"), "staged change\n").unwrap();
        stage_file(&repo, "tracked.txt").unwrap();
        fs::write(dir.path().join("untracked.txt"), "unstaged\n").unwrap();

        let snapshot = capture(&repo).unwrap();

        // Blow away all working-tree/index state.
        stage_commit(&repo, "unrelated commit", false, false).unwrap();
        fs::remove_file(dir.path().join("untracked.txt")).unwrap();

        restore(&mut repo, &snapshot).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
            "staged change\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("untracked.txt")).unwrap(),
            "unstaged\n"
        );
        let staged = crate::diff::diff_staged(&repo).unwrap();
        assert!(staged
            .iter()
            .any(|d| d.new_path.as_deref() == Some("tracked.txt")));
        let unstaged = crate::diff::diff_unstaged(&repo).unwrap();
        assert!(unstaged
            .iter()
            .any(|d| d.new_path.as_deref() == Some("untracked.txt")));
    }

    #[test]
    fn keep_alive_oids_includes_every_captured_ref_target() {
        let (_dir, repo) = repo_init();
        create_branch(&repo, "feature", None).unwrap();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        let snapshot = capture(&repo).unwrap();
        let oids = keep_alive_oids(&snapshot);

        let feature_oid = repo
            .find_branch("feature", BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        assert!(oids.contains(&feature_oid));
        let head_oid = repo.head().unwrap().target().unwrap();
        assert!(
            oids.contains(&head_oid),
            "main's own tip should be captured too"
        );
    }
}
