// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Writing git's own `commit-graph` acceleration file — distinct from PushGit's own
//! branch-color cache (`./color_cache.rs`). Shells out to the real `git` binary: libgit2
//! itself has a full C writer API for this (`git2/sys/commit_graph.h` — `git_commit_graph_
//! writer_new`/`_add_revwalk`/`_commit`), but the safe `git2` crate this project uses
//! everywhere else never wraps it, matching the exact situation `remote/` already solved
//! for clone/fetch/pull/push by shelling out instead of reaching for raw libgit2-sys FFI.

use std::path::Path;

use crate::error::PushGitResult;
use crate::remote::run_git;

/// Below this commit count, walking without a `commit-graph` file is already fast enough
/// that writing one isn't worth the one-time cost — this is the
/// "~5,000 commits" guidance.
pub const LARGE_REPO_COMMIT_THRESHOLD: usize = 5_000;

/// Whether writing a commit-graph file is worth doing right now: the repo is large enough,
/// and doesn't already have one. Split out from `exists`/`write` (which touch the
/// filesystem/spawn a process) so the threshold decision itself is trivial to test.
pub fn should_write(commit_count: usize, already_exists: bool) -> bool {
    !already_exists && commit_count > LARGE_REPO_COMMIT_THRESHOLD
}

/// Whether `git_dir` (a repository's `.git` directory, e.g. `Repository::path()`) already
/// has a `commit-graph` file — a plain file for a single-file graph, or a
/// `commit-graph-chain` file for a split/incremental one.
pub fn exists(git_dir: &Path) -> bool {
    let info_dir = git_dir.join("objects").join("info");
    info_dir.join("commit-graph").exists() || info_dir.join("commit-graph-chain").exists()
}

/// Writes a commit-graph file covering every commit reachable from any ref. Intended to be
/// spawned as a detached background task by the caller (`GraphSessions::open`) — never
/// awaited inline, so it can never delay the graph render that triggered it
/// ("Don't block the first graph render on this").
pub async fn write(workdir: &Path) -> PushGitResult<()> {
    run_git(&["commit-graph", "write", "--reachable"], Some(workdir)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;

    #[test]
    fn should_write_when_large_and_missing() {
        assert!(should_write(10_000, false));
    }

    #[test]
    fn should_not_write_when_already_present() {
        assert!(!should_write(10_000, true));
    }

    #[test]
    fn should_not_write_when_small() {
        assert!(!should_write(100, false));
    }

    #[test]
    fn should_write_is_exclusive_at_the_threshold_boundary() {
        assert!(
            !should_write(LARGE_REPO_COMMIT_THRESHOLD, false),
            "exactly at the threshold isn't over it yet"
        );
        assert!(should_write(LARGE_REPO_COMMIT_THRESHOLD + 1, false));
    }

    #[test]
    fn exists_is_false_for_a_fresh_repo_with_no_commit_graph_file() {
        let (_dir, repo) = repo_init();

        assert!(!exists(repo.path()));
    }

    #[tokio::test]
    async fn write_creates_a_real_commit_graph_file_git_recognizes() {
        let (dir, repo) = repo_init();

        write(dir.path()).await.unwrap();

        assert!(exists(repo.path()));
    }
}
