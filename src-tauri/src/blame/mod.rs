// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Blame (per-line authorship) and file history — secondary
//! investigative tools, not part of the core commit/diff/merge loop. Both start from HEAD
//! unless given a revision.

mod model;

pub use model::{BlameLine, FileHistoryEntry};

use std::path::Path;

use git2::Repository;

use crate::error::PushGitResult;

/// Bounds file-history's worst case on a huge repo where a path was touched rarely (or
/// never) — examines at most this many commits regardless of how many history entries
/// that turns up, not just how many entries are returned.
const MAX_COMMITS_EXAMINED: usize = 5000;
const MAX_HISTORY_ENTRIES: usize = 200;

fn start_commit<'r>(repo: &'r Repository, rev: Option<&str>) -> PushGitResult<git2::Commit<'r>> {
    Ok(match rev {
        Some(rev) => repo.revparse_single(rev)?.peel_to_commit()?,
        None => repo.head()?.peel_to_commit()?,
    })
}

/// Per-line authorship for `path` as of `rev` (HEAD when `None`).
pub fn blame_file(
    repo: &Repository,
    path: &str,
    rev: Option<&str>,
) -> PushGitResult<Vec<BlameLine>> {
    let start = start_commit(repo, rev)?;
    let mut options = git2::BlameOptions::new();
    options.newest_commit(start.id());
    let blame = repo.blame_file(Path::new(path), Some(&mut options))?;

    let blob = start
        .tree()?
        .get_path(Path::new(path))?
        .to_object(repo)?
        .peel_to_blob()?;
    let content = String::from_utf8_lossy(blob.content()).into_owned();

    let mut lines = Vec::new();
    for (index, content_line) in content.lines().enumerate() {
        let line_no = index as u32 + 1;
        let Some(hunk) = blame.get_line(line_no as usize) else {
            continue;
        };
        let commit_oid = hunk.final_commit_id();
        let commit = repo.find_commit(commit_oid)?;
        let oid_str = commit_oid.to_string();
        let short_oid = oid_str[..7.min(oid_str.len())].to_string();

        lines.push(BlameLine {
            line_no,
            content: content_line.to_string(),
            oid: oid_str,
            short_oid,
            author_name: commit.author().name().unwrap_or("Unknown").to_string(),
            author_time: commit.author().when().seconds(),
            summary: commit.summary().unwrap_or_default().to_string(),
        });
    }
    Ok(lines)
}

/// The commits that changed `path`, newest first — a path is considered "changed" by a
/// commit if its tree differs from *any* parent's tree at that path (matching plain `git
/// log -- path`'s usual heuristic for merge commits); a root commit changes it if the path
/// exists in its tree at all. Rename-following (`git log --follow`) isn't implemented.
pub fn file_history(
    repo: &Repository,
    path: &str,
    rev: Option<&str>,
) -> PushGitResult<Vec<FileHistoryEntry>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push(start_commit(repo, rev)?.id())?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

    let mut entries = Vec::new();

    for (examined, oid) in revwalk.enumerate() {
        if entries.len() >= MAX_HISTORY_ENTRIES || examined >= MAX_COMMITS_EXAMINED {
            break;
        }

        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        let changed = if commit.parent_count() == 0 {
            tree.get_path(Path::new(path)).is_ok()
        } else {
            (0..commit.parent_count()).try_fold(false, |touched, i| -> PushGitResult<bool> {
                if touched {
                    return Ok(true);
                }
                let parent_tree = commit.parent(i)?.tree()?;
                let mut diff_opts = git2::DiffOptions::new();
                diff_opts.pathspec(path);
                let diff =
                    repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), Some(&mut diff_opts))?;
                Ok(diff.deltas().len() > 0)
            })?
        };

        if changed {
            let oid_str = oid.to_string();
            entries.push(FileHistoryEntry {
                short_oid: oid_str[..7.min(oid_str.len())].to_string(),
                oid: oid_str,
                summary: commit.summary().unwrap_or_default().to_string(),
                author_name: commit.author().name().unwrap_or("Unknown").to_string(),
                author_time: commit.author().when().seconds(),
            });
        }
    }

    Ok(entries)
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
    fn blame_attributes_each_line_to_the_commit_that_last_touched_it() {
        let (_dir, repo) = repo_init();
        let first = commit_file(&repo, "a.txt", "line one\nline two\n");
        let second = commit_file(&repo, "a.txt", "line one\nline two changed\n");

        let lines = blame_file(&repo, "a.txt", None).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_no, 1);
        assert_eq!(lines[0].content, "line one");
        assert_eq!(lines[0].oid, first.to_string());
        assert_eq!(lines[1].line_no, 2);
        assert_eq!(lines[1].content, "line two changed");
        assert_eq!(lines[1].oid, second.to_string());
    }

    #[test]
    fn file_history_lists_only_commits_that_touched_the_path_newest_first() {
        let (_dir, repo) = repo_init();
        let first = commit_file(&repo, "a.txt", "a\n");
        commit_file(&repo, "unrelated.txt", "unrelated\n");
        let third = commit_file(&repo, "a.txt", "a changed\n");

        let history = file_history(&repo, "a.txt", None).unwrap();

        let oids: Vec<String> = history.iter().map(|e| e.oid.clone()).collect();
        assert_eq!(oids, vec![third.to_string(), first.to_string()]);
    }

    #[test]
    fn blame_and_history_start_from_the_given_revision() {
        let (_dir, repo) = repo_init();
        let first = commit_file(&repo, "a.txt", "line one\n");
        commit_file(&repo, "a.txt", "line one changed\n");

        let lines = blame_file(&repo, "a.txt", Some(&first.to_string())).unwrap();
        assert_eq!(lines[0].content, "line one");
        assert_eq!(lines[0].oid, first.to_string());

        let history = file_history(&repo, "a.txt", Some(&first.to_string())).unwrap();
        let oids: Vec<String> = history.iter().map(|e| e.oid.clone()).collect();
        assert_eq!(oids, vec![first.to_string()]);
    }

    #[test]
    fn file_history_is_empty_for_a_path_never_touched() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "a.txt", "a\n");

        let history = file_history(&repo, "never-existed.txt", None).unwrap();

        assert!(history.is_empty());
    }
}
