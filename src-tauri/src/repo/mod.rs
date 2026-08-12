// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Repository discovery and open, multi-repo/tab session management, and `.git` state
//! (HEAD, current branch, detached-HEAD state).

use std::path::{Path, PathBuf};

use git2::Repository;

use crate::error::{PushGitError, PushGitResult};

/// Opens the git repository at `path`, discovering upward through parent directories
/// the same way `git` itself does (so opening from a subdirectory of a repo works).
pub fn open(path: &Path) -> PushGitResult<Repository> {
    Ok(Repository::discover(path)?)
}

/// The repository's working directory — errors for a bare repository, which none of
/// PushGit's operations target. Shared by `interactive_rebase`/`cherry_pick_range`, which
/// both need a plain filesystem path to hand to a `git` subprocess's `current_dir`.
pub fn workdir_of(path: &Path) -> PushGitResult<PathBuf> {
    open(path)?
        .workdir()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| PushGitError::Invalid("repository has no working directory".to_string()))
}

/// The repository's `.git` directory — for callers that need to check on-disk state
/// (`rebase-merge`, `CHERRY_PICK_HEAD`) `git2` has no query for directly.
pub fn git_dir_of(path: &Path) -> PushGitResult<PathBuf> {
    Ok(open(path)?.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;

    #[test]
    fn opens_a_valid_repository() {
        let (dir, _repo) = repo_init();

        let opened = open(dir.path()).expect("open should succeed on a valid repo");
        assert!(opened.path().starts_with(dir.path()));
    }

    #[test]
    fn discovers_repository_from_a_subdirectory() {
        let (dir, _repo) = repo_init();
        let subdir = dir.path().join("some/nested/dir");
        std::fs::create_dir_all(&subdir).unwrap();

        let opened = open(&subdir).expect("open should discover the repo upward");
        assert!(opened.path().starts_with(dir.path()));
    }

    #[test]
    fn errors_on_a_non_repository_path() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(open(dir.path()).is_err());
    }
}
