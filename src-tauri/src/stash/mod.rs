// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Stash create/apply/drop, plus rename and selective (per-file) stash.
//!
//! git2's stash functions require `&mut Repository` (unlike almost everything else in this
//! crate, which takes `&Repository`) — libgit2's stash implementation needs exclusive
//! access to the repository handle.
//!
//! **Hunk-level (sub-file) stash is deliberately not implemented.** libgit2 only scopes
//! `stash_save` by whole file (`StashSaveOptions::pathspec`, used by
//! `create_stash_for_paths` below) — there is no hunk-granularity primitive anywhere in its
//! stash API. Building one would mean hand-constructing git's own multi-parent stash commit
//! format from scratch (matching the exact parent/tree structure `git stash`'s own
//! `stash_save` produces) well enough for libgit2's `stash_apply`/`stash_pop` to interpret
//! it correctly later — a real but genuinely risky undertaking, since getting that internal
//! structure subtly wrong wouldn't fail loudly, it would silently corrupt or lose a user's
//! stashed changes the next time they tried to restore them. That risk profile — a rare,
//! hard-to-test failure mode with real data loss on the other side — is why file-level
//! selective stash ships here instead, not a scope call made lightly.

use serde::Serialize;

use git2::{Repository, StashFlags, StashSaveOptions};

use crate::error::{PushGitError, PushGitResult};
use crate::stage;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub oid: String,
}

/// Stashes the working directory (and staged changes) with an optional message.
/// Includes untracked files — unlike plain `git stash`'s default, a GUI
/// "stash everything" action shouldn't silently leave new files behind.
pub fn create_stash(repo: &mut Repository, message: Option<&str>) -> PushGitResult<String> {
    let signature = repo.signature()?;
    let oid = repo.stash_save(
        &signature,
        message.unwrap_or("WIP"),
        Some(StashFlags::INCLUDE_UNTRACKED),
    )?;
    Ok(oid.to_string())
}

/// Stashes only the changes to `paths` (including untracked ones among them), leaving
/// every other file's uncommitted changes exactly as they were — `git stash push --
/// <paths>`'s equivalent via `StashSaveOptions::pathspec`. See the module doc for why this
/// stops at file granularity rather than hunk granularity.
///
/// Two libgit2 quirks this works around, found by testing against a real repo rather than
/// assumed from the API surface alone:
///
/// 1. **The pathspec only scopes what's *captured*, not what's *cleared* afterward.**
///    `git_stash_save_with_opts`'s own post-save cleanup step resets the *entire* working
///    directory and index back to HEAD once a stash is created — regardless of `paths` —
///    unless `StashFlags::KEEP_ALL` is also set, which instead resets *nothing* at all. So
///    the pathspec-scoped stash commit is created correctly, but neither flag combination
///    alone gets "only the selected paths disappear from view" — this passes `KEEP_ALL`
///    (skipping libgit2's own indiscriminate reset) and then explicitly reverts just
///    `paths` itself afterward via `stage::discard_file_changes`, the same function the
///    ordinary staging UI's "Discard" button already uses and already tests.
/// 2. **`StashSaveOptions` has no public setter for a custom message** (the field exists
///    on the struct, but nothing populates it), so a fresh stash always lands with
///    libgit2's own default message; when `message` is given, this immediately renames the
///    just-created entry (always index 0 — a new stash is always the newest) via
///    `rename_stash`, reusing its reflog-rewrite rather than duplicating it.
pub fn create_stash_for_paths(
    repo: &mut Repository,
    message: Option<&str>,
    paths: &[String],
) -> PushGitResult<String> {
    let signature = repo.signature()?;
    let mut options = StashSaveOptions::new(signature);
    options.flags(Some(StashFlags::INCLUDE_UNTRACKED | StashFlags::KEEP_ALL));
    for path in paths {
        options.pathspec(path.as_str());
    }
    let oid = repo.stash_save_ext(Some(&mut options))?;

    // `KEEP_ALL` (above) left every path's on-disk content exactly as it was before this
    // call, so each one is still there to discard now — `discard_file_changes` already
    // handles "no HEAD content" (a path that was only ever untracked) by deleting it
    // instead of reverting it.
    for path in paths {
        stage::discard_file_changes(repo, path)?;
    }

    if let Some(message) = message {
        rename_stash(repo, 0, message)?;
    }

    Ok(oid.to_string())
}

/// Renames a stash by rewriting `refs/stash`'s reflog with the same oids in the same order,
/// substituting just the message at `index` — a stash's "name" *is* its reflog message,
/// and there's no dedicated libgit2 API to change one in place, only to read/write the
/// whole reflog (the same technique `undo::restore_stash` uses to rebuild it after an
/// undo/redo). The oids themselves are untouched, so this never risks the stash's content.
pub fn rename_stash(repo: &mut Repository, index: usize, new_message: &str) -> PushGitResult<()> {
    let mut entries = Vec::new();
    repo.stash_foreach(|i, message, oid| {
        entries.push((
            *oid,
            if i == index {
                new_message.to_string()
            } else {
                message.to_string()
            },
        ));
        true
    })?;

    let Some((top_oid, _)) = entries.first() else {
        return Err(PushGitError::Invalid("no stashes to rename".to_string()));
    };
    if index >= entries.len() {
        return Err(PushGitError::Invalid(format!("no stash at index {index}")));
    }

    repo.reference("refs/stash", *top_oid, true, "pushgit: rename stash")?;

    let mut reflog = repo.reflog("refs/stash")?;
    while !reflog.is_empty() {
        reflog.remove(0, false)?;
    }
    let signature = repo.signature()?;
    // Reflog order is oldest-first; `stash_foreach`'s index is newest-first (0 = top).
    for (oid, message) in entries.iter().rev() {
        reflog.append(*oid, &signature, Some(message))?;
    }
    reflog.write()?;
    Ok(())
}

pub fn list_stashes(repo: &mut Repository) -> PushGitResult<Vec<StashEntry>> {
    let mut entries = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        entries.push(StashEntry {
            index,
            message: message.to_string(),
            oid: oid.to_string(),
        });
        true
    })?;
    Ok(entries)
}

/// Applies a stash without removing it from the stash list.
pub fn apply_stash(repo: &mut Repository, index: usize) -> PushGitResult<()> {
    repo.stash_apply(index, None)?;
    Ok(())
}

/// Applies a stash and removes it from the stash list.
pub fn pop_stash(repo: &mut Repository, index: usize) -> PushGitResult<()> {
    repo.stash_pop(index, None)?;
    Ok(())
}

/// Removes a stash without applying it — a destructive, unrecoverable action the frontend
/// should confirm with the user first.
pub fn drop_stash(repo: &mut Repository, index: usize) -> PushGitResult<()> {
    repo.stash_drop(index)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;
    use std::path::Path;

    #[test]
    fn create_list_and_drop_a_stash() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

        let oid = create_stash(&mut repo, Some("wip: testing")).unwrap();
        assert!(!oid.is_empty());
        assert!(
            !dir.path().join("dirty.txt").exists(),
            "stash should clean the working tree"
        );

        let stashes = list_stashes(&mut repo).unwrap();
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].message, "On main: wip: testing");

        drop_stash(&mut repo, 0).unwrap();
        assert!(list_stashes(&mut repo).unwrap().is_empty());
    }

    #[test]
    fn apply_restores_the_working_tree_without_removing_the_stash() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        create_stash(&mut repo, None).unwrap();

        apply_stash(&mut repo, 0).unwrap();

        assert!(dir.path().join("dirty.txt").exists());
        assert_eq!(
            list_stashes(&mut repo).unwrap().len(),
            1,
            "apply should not remove the stash"
        );
    }

    #[test]
    fn pop_restores_the_working_tree_and_removes_the_stash() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        create_stash(&mut repo, None).unwrap();

        pop_stash(&mut repo, 0).unwrap();

        assert!(dir.path().join("dirty.txt").exists());
        assert!(list_stashes(&mut repo).unwrap().is_empty());
    }

    #[test]
    fn stash_also_covers_staged_changes() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("staged.txt"), "staged\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();

        create_stash(&mut repo, None).unwrap();

        assert!(!dir.path().join("staged.txt").exists());
        let diffs = crate::diff::diff_staged(&repo).unwrap();
        assert!(
            diffs.is_empty(),
            "staged changes should be stashed away too"
        );
    }

    #[test]
    fn rename_stash_rewrites_the_message_keeping_the_same_content() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let oid = create_stash(&mut repo, Some("original")).unwrap();

        rename_stash(&mut repo, 0, "renamed").unwrap();

        let entries = list_stashes(&mut repo).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "renamed");
        assert_eq!(
            entries[0].oid, oid,
            "renaming must not change the stashed content"
        );
    }

    #[test]
    fn rename_stash_only_touches_the_requested_index() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        create_stash(&mut repo, Some("first")).unwrap();
        fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        create_stash(&mut repo, Some("second")).unwrap();

        rename_stash(&mut repo, 0, "renamed-second").unwrap();

        let entries = list_stashes(&mut repo).unwrap();
        assert_eq!(entries[0].message, "renamed-second");
        // git's own `stash_save` always prepends "On <branch>: " to whatever message is
        // given — not something `rename_stash` does, so the untouched entry keeps it too.
        assert_eq!(entries[1].message, "On main: first");
    }

    #[test]
    fn rename_stash_errors_for_an_out_of_range_index() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        create_stash(&mut repo, None).unwrap();

        assert!(rename_stash(&mut repo, 5, "x").is_err());
    }

    #[test]
    fn create_stash_for_paths_only_stashes_the_given_files() {
        let (dir, mut repo) = repo_init();
        // Both files must exist in HEAD first — pathspec scoping applies cleanly to
        // modified tracked files; scoping brand-new untracked files by pathspec isn't
        // exercised here.
        for name in ["a.txt", "b.txt"] {
            fs::write(dir.path().join(name), "original\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new(name)).unwrap();
            index.write().unwrap();
        }
        let tree_oid = repo.index().unwrap().write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_oid).unwrap();
            let sig = repo.signature().unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(
                Some("HEAD"),
                &sig,
                &sig,
                "add a.txt and b.txt",
                &tree,
                &[&head],
            )
            .unwrap();
        }

        fs::write(dir.path().join("a.txt"), "a changed\n").unwrap();
        fs::write(dir.path().join("b.txt"), "b changed\n").unwrap();

        create_stash_for_paths(&mut repo, Some("just a"), &["a.txt".to_string()]).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "original\n",
            "the selected file's change should be stashed away"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "b changed\n",
            "the other file's change should be left alone"
        );
        let entries = list_stashes(&mut repo).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "just a");
    }

    #[test]
    fn create_stash_for_paths_removes_a_selected_untracked_file() {
        let (dir, mut repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "new\n").unwrap();
        fs::write(dir.path().join("other.txt"), "other\n").unwrap();

        create_stash_for_paths(&mut repo, None, &["new.txt".to_string()]).unwrap();

        assert!(
            !dir.path().join("new.txt").exists(),
            "the selected untracked file should be stashed away, not just left modified"
        );
        assert!(
            dir.path().join("other.txt").exists(),
            "the other untracked file should be left alone"
        );
    }
}
