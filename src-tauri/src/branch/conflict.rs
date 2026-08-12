// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Merge-conflict listing and resolution, backing the 3-way conflict editor.

use git2::Repository;

use super::invalid;
use crate::diff::{self, ConflictSides};
use crate::error::PushGitResult;

pub fn list_conflicts(repo: &Repository) -> PushGitResult<Vec<String>> {
    super::collect_conflict_paths(&repo.index()?)
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
    fn resolve_conflict_stages_the_working_tree_content() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        crate::branch::merge_branch(&repo, "feature").unwrap();

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
        crate::branch::merge_branch(&repo, "feature").unwrap();
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

    #[test]
    fn conflict_sides_reports_a_missing_side_as_none_for_a_delete_modify_conflict() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_delete_file(&repo, "shared.txt");

        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        crate::branch::merge_branch(&repo, "feature").unwrap();

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
        crate::branch::merge_branch(&repo, "feature").unwrap();

        resolve_conflict_as_deleted(&repo, "shared.txt").unwrap();

        assert!(!dir.path().join("shared.txt").exists());
        assert!(list_conflicts(&repo).unwrap().is_empty());
    }
}
