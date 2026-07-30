// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Repository discovery and open, multi-repo/tab session management, and `.git` state
//! (HEAD, current branch, detached-HEAD state).

use std::path::Path;

use git2::Repository;

use crate::error::PushGitResult;

/// Opens the git repository at `path`, discovering upward through parent directories
/// the same way `git` itself does (so opening from a subdirectory of a repo works).
pub fn open(path: &Path) -> PushGitResult<Repository> {
    Ok(Repository::discover(path)?)
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
