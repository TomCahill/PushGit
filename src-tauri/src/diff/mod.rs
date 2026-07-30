// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Diff and patch computation via git2's `Diff`/`Patch` types, binary/large-file detection,
//! hunk parsing feeding the diff viewer.

mod model;

pub use model::{ConflictSides, FileDiff, FileStatus, Hunk, Line, LineOrigin};

use git2::{Blob, Delta, Diff, DiffLineType, DiffOptions, Patch, Repository};

use crate::error::PushGitResult;

/// Working-directory changes not yet staged (workdir vs. index). Includes untracked files
/// — a git GUI's "unstaged changes" list needs to offer brand-new files for staging too,
/// not just modifications to already-tracked ones.
pub fn diff_unstaged(repo: &Repository) -> PushGitResult<Vec<FileDiff>> {
    let mut opts = DiffOptions::new();
    // `include_untracked`/`recurse_untracked_dirs` alone only get an untracked file *listed*
    // (status + size, no hunks) — `show_untracked_content` is the separate flag that makes
    // git2 actually generate its line-by-line patch, exactly like an added/modified file.
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);

    let diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
    build_file_diffs(&diff)
}

/// Staged changes (index vs. HEAD).
pub fn diff_staged(repo: &Repository) -> PushGitResult<Vec<FileDiff>> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, None)?;
    build_file_diffs(&diff)
}

/// A commit's changes against its first parent (or against an empty tree for a root
/// commit), for the "diff against HEAD"/click-a-commit flow.
pub fn diff_commit(repo: &Repository, commit_oid: &str) -> PushGitResult<Vec<FileDiff>> {
    let commit = repo.revparse_single(commit_oid)?.peel_to_commit()?;
    let new_tree = commit.tree()?;
    let old_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = repo.diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)?;
    build_file_diffs(&diff)
}

/// A commit's changes against the live working directory, for the "diff against working
/// tree" context-menu action. Uses `_with_index` so staged
/// deletes/etc. are blended in exactly like plain `git diff <rev>` — the plain
/// `diff_tree_to_workdir` variant explicitly documents that it is *not* equivalent to that
/// command. Untracked files are correctly excluded by the default `DiffOptions` (unlike
/// `diff_unstaged`, which has to opt into them), matching `git diff <rev>`'s own behavior.
pub fn diff_commit_to_workdir(repo: &Repository, commit_oid: &str) -> PushGitResult<Vec<FileDiff>> {
    let tree = repo.revparse_single(commit_oid)?.peel_to_commit()?.tree()?;
    let diff = repo.diff_tree_to_workdir_with_index(Some(&tree), None)?;
    build_file_diffs(&diff)
}

/// The diff between two arbitrary commits, for the ctrl-click-two-commits flow.
pub fn diff_between_commits(
    repo: &Repository,
    from_oid: &str,
    to_oid: &str,
) -> PushGitResult<Vec<FileDiff>> {
    let from_tree = repo.revparse_single(from_oid)?.peel_to_commit()?.tree()?;
    let to_tree = repo.revparse_single(to_oid)?.peel_to_commit()?.tree()?;

    let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;
    build_file_diffs(&diff)
}

fn build_file_diffs(diff: &Diff) -> PushGitResult<Vec<FileDiff>> {
    let mut results = Vec::with_capacity(diff.deltas().len());

    for i in 0..diff.deltas().len() {
        let delta = diff.get_delta(i).expect("index within deltas().len()");
        let is_binary = delta.flags().is_binary();
        let status = map_status(delta.status());
        let old_path = delta.old_file().path().map(|p| p.display().to_string());
        let new_path = delta.new_file().path().map(|p| p.display().to_string());

        let hunks = if is_binary {
            Vec::new()
        } else {
            build_hunks(diff, i)?
        };
        let (insertions, deletions) = count_stats(&hunks);

        results.push(FileDiff {
            old_path,
            new_path,
            status,
            is_binary,
            hunks,
            insertions,
            deletions,
        });
    }

    Ok(results)
}

/// Counts addition/deletion lines across every hunk. Derived from the hunks already built
/// rather than a second `diff.stats()` call — that API only reports aggregate totals for
/// the whole `Diff`, not split per file, so it can't answer "how big was just this file's
/// change" any cheaper than counting what's already in memory.
fn count_stats(hunks: &[Hunk]) -> (u32, u32) {
    let mut insertions = 0;
    let mut deletions = 0;
    for line in hunks.iter().flat_map(|h| &h.lines) {
        match line.origin {
            LineOrigin::Addition => insertions += 1,
            LineOrigin::Deletion => deletions += 1,
            LineOrigin::Context => {}
        }
    }
    (insertions, deletions)
}

fn build_hunks(diff: &Diff, delta_index: usize) -> PushGitResult<Vec<Hunk>> {
    let Some(patch) = Patch::from_diff(diff, delta_index)? else {
        return Ok(Vec::new());
    };
    hunks_from_patch(&patch)
}

/// Diffs two blobs directly, independent of any tree/index/workdir state — used for
/// merge-conflict side comparisons, where the content being compared lives only in the
/// index's conflict stages, not in any commit or the working tree. `None` for a side means
/// "absent" (e.g. an add/add conflict has no base) and is diffed against an empty blob, so
/// the other side's entire content shows up as one hunk of additions/deletions.
pub fn diff_blob_content(
    repo: &Repository,
    old: Option<&Blob>,
    new: Option<&Blob>,
) -> PushGitResult<(Vec<Hunk>, bool)> {
    let empty_blob = repo.find_blob(repo.blob(&[])?)?;
    let old_blob = old.unwrap_or(&empty_blob);
    let new_blob = new.unwrap_or(&empty_blob);

    let patch = Patch::from_blobs(old_blob, None, new_blob, None, None)?;
    let is_binary = patch.delta().flags().is_binary();
    let hunks = if is_binary {
        Vec::new()
    } else {
        hunks_from_patch(&patch)?
    };
    Ok((hunks, is_binary))
}

fn hunks_from_patch(patch: &Patch) -> PushGitResult<Vec<Hunk>> {
    let mut hunks = Vec::with_capacity(patch.num_hunks());

    for h in 0..patch.num_hunks() {
        let (raw_hunk, line_count) = patch.hunk(h)?;
        let mut lines = Vec::with_capacity(line_count);

        for l in 0..line_count {
            let raw_line = patch.line_in_hunk(h, l)?;
            lines.push(Line {
                origin: map_origin(raw_line.origin_value()),
                content: String::from_utf8_lossy(raw_line.content()).into_owned(),
                old_lineno: raw_line.old_lineno(),
                new_lineno: raw_line.new_lineno(),
            });
        }

        hunks.push(Hunk {
            header: String::from_utf8_lossy(raw_hunk.header())
                .trim_end()
                .to_string(),
            old_start: raw_hunk.old_start(),
            old_lines: raw_hunk.old_lines(),
            new_start: raw_hunk.new_start(),
            new_lines: raw_hunk.new_lines(),
            lines,
        });
    }

    Ok(hunks)
}

fn map_status(status: Delta) -> FileStatus {
    match status {
        Delta::Added => FileStatus::Added,
        Delta::Deleted => FileStatus::Deleted,
        Delta::Modified => FileStatus::Modified,
        Delta::Renamed => FileStatus::Renamed,
        Delta::Copied => FileStatus::Copied,
        Delta::Typechange => FileStatus::Typechange,
        Delta::Conflicted => FileStatus::Conflicted,
        Delta::Untracked => FileStatus::Untracked,
        _ => FileStatus::Unreadable,
    }
}

fn map_origin(origin: DiffLineType) -> LineOrigin {
    match origin {
        DiffLineType::Addition => LineOrigin::Addition,
        DiffLineType::Deletion => LineOrigin::Deletion,
        _ => LineOrigin::Context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;

    #[test]
    fn unstaged_diff_reports_a_modified_file() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add readme", &tree, &[&head])
            .unwrap();

        fs::write(dir.path().join("README.md"), "hello\nworld\nagain\n").unwrap();

        let diffs = diff_unstaged(&repo).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Modified);
        assert!(!diffs[0].is_binary);
        assert!(diffs[0].hunks.iter().any(|h| h
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Addition && l.content.contains("again"))));
    }

    #[test]
    fn unstaged_diff_reports_hunk_content_for_a_brand_new_untracked_file() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "brand new content\n").unwrap();

        let diffs = diff_unstaged(&repo).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Untracked);
        assert!(!diffs[0].is_binary);
        assert!(
            diffs[0].hunks.iter().any(|h| h.lines.iter().any(
                |l| l.origin == LineOrigin::Addition && l.content.contains("brand new content")
            )),
            "expected the untracked file's own content to show up as addition lines, got: {:?}",
            diffs[0].hunks
        );
    }

    #[test]
    fn file_diff_reports_insertion_and_deletion_counts() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add a.txt", &tree, &[&head])
            .unwrap();

        // Replaces the single "two" line with two new lines — one deletion, two insertions
        // — while "one"/"three" stay as unchanged context lines.
        fs::write(dir.path().join("a.txt"), "one\nfour\nfive\nthree\n").unwrap();

        let diffs = diff_unstaged(&repo).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].insertions, 2);
        assert_eq!(diffs[0].deletions, 1);
    }

    #[test]
    fn staged_diff_reports_a_newly_added_file() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "brand new\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("new.txt")).unwrap();
        index.write().unwrap();

        let diffs = diff_staged(&repo).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Added);
    }

    #[test]
    fn commit_diff_against_first_parent() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "add a.txt", &tree, &[&head])
            .unwrap();

        let diffs = diff_commit(&repo, &commit_oid.to_string()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].new_path.as_deref(), Some("a.txt"));
        assert_eq!(diffs[0].status, FileStatus::Added);
    }

    #[test]
    fn root_commit_diffs_against_an_empty_tree() {
        let (_dir, repo) = repo_init();
        let head_oid = repo.head().unwrap().target().unwrap();

        let diffs = diff_commit(&repo, &head_oid.to_string()).unwrap();

        // repo_init's initial commit has an (empty) tree and no files — nothing to diff,
        // but this must not error just because there's no parent to diff against.
        assert!(diffs.is_empty());
    }

    #[test]
    fn diff_commit_to_workdir_reports_uncommitted_changes_since_that_commit() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "add a.txt", &tree, &[&head])
            .unwrap();

        fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();

        let diffs = diff_commit_to_workdir(&repo, &commit_oid.to_string()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Modified);
        assert!(diffs[0].hunks.iter().any(|h| h
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Addition && l.content.contains("two"))));
    }

    #[test]
    fn diff_commit_to_workdir_excludes_untracked_files() {
        let (dir, repo) = repo_init();
        let head_oid = repo.head().unwrap().target().unwrap();

        fs::write(dir.path().join("new.txt"), "brand new\n").unwrap();

        let diffs = diff_commit_to_workdir(&repo, &head_oid.to_string()).unwrap();

        assert!(
            diffs.is_empty(),
            "untracked files must not appear, matching plain `git diff <rev>`, got: {:?}",
            diffs
        );
    }

    #[test]
    fn diff_commit_to_workdir_blends_a_staged_delete_with_a_recreated_workdir_file() {
        // The exact scenario git2's own doc comment uses to distinguish
        // `diff_tree_to_workdir_with_index` from plain `diff_tree_to_workdir`: a staged
        // deletion, then the file is put back into the working dir and further modified
        // without being re-staged. Plain `diff_tree_to_workdir` (ignoring the index
        // entirely) would report this path as "modified"; real `git diff <tree>` — what
        // `diff_commit_to_workdir` must match — reports it as "deleted", since the staged
        // delete dominates and the unstaged recreation is invisible to the index, exactly
        // like an ordinary untracked file.
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "base\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "add a.txt", &tree, &[&head])
            .unwrap();

        fs::remove_file(dir.path().join("a.txt")).unwrap();
        index.remove_path(std::path::Path::new("a.txt")).unwrap();
        index.write().unwrap();
        fs::write(dir.path().join("a.txt"), "recreated\n").unwrap();

        let diffs = diff_commit_to_workdir(&repo, &commit_oid.to_string()).unwrap();

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Deleted);
    }

    fn blob<'a>(repo: &'a Repository, content: &str) -> git2::Blob<'a> {
        let oid = repo.blob(content.as_bytes()).unwrap();
        repo.find_blob(oid).unwrap()
    }

    #[test]
    fn diff_blob_content_reports_the_changed_lines_between_two_blobs() {
        let (_dir, repo) = repo_init();
        let old = blob(&repo, "a\nb\nc\n");
        let new = blob(&repo, "a\nb2\nc\n");

        let (hunks, is_binary) = diff_blob_content(&repo, Some(&old), Some(&new)).unwrap();

        assert!(!is_binary);
        assert!(hunks.iter().any(|h| h
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Deletion && l.content.contains('b'))));
        assert!(hunks.iter().any(|h| h
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Addition && l.content.contains("b2"))));
    }

    #[test]
    fn diff_blob_content_treats_a_missing_side_as_empty() {
        let (_dir, repo) = repo_init();
        let new = blob(&repo, "brand new\n");

        let (hunks, is_binary) = diff_blob_content(&repo, None, Some(&new)).unwrap();

        assert!(!is_binary);
        assert!(hunks.iter().any(|h| h
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Addition && l.content.contains("brand new"))));
    }

    #[test]
    fn diff_blob_content_detects_binary_and_returns_no_hunks() {
        let (_dir, repo) = repo_init();
        let old = repo
            .find_blob(repo.blob(&[0, 1, 2, 0, 3]).unwrap())
            .unwrap();
        let new = repo
            .find_blob(repo.blob(&[0, 1, 2, 0, 4]).unwrap())
            .unwrap();

        let (hunks, is_binary) = diff_blob_content(&repo, Some(&old), Some(&new)).unwrap();

        assert!(is_binary);
        assert!(hunks.is_empty());
    }

    #[test]
    fn unstaged_diff_reports_conflicted_status_for_an_unresolved_merge_conflict() {
        use crate::branch::{checkout_branch, create_branch, merge_branch};

        let (dir, repo) = repo_init();
        fs::write(dir.path().join("shared.txt"), "base\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[&head])
            .unwrap();

        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();
        fs::write(dir.path().join("shared.txt"), "feature version\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "feature change", &tree, &[&head])
            .unwrap();
        checkout_branch(&repo, "main").unwrap();
        fs::write(dir.path().join("shared.txt"), "main version\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "main change", &tree, &[&head])
            .unwrap();

        let outcome = merge_branch(&repo, "feature").unwrap();
        assert!(matches!(
            outcome,
            crate::branch::MergeOutcome::Conflicts { .. }
        ));

        let diffs = diff_unstaged(&repo).unwrap();
        let conflicted = diffs
            .iter()
            .find(|d| d.new_path.as_deref() == Some("shared.txt"));
        assert_eq!(conflicted.unwrap().status, FileStatus::Conflicted);
    }
}
