// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fixture repositories built as code, not checked in as binary `.git` blobs.
//! Test-only — not compiled into the shipped binary.

use git2::{Repository, RepositoryInitOptions};
use tempfile::TempDir;

/// Sets repo-local `user.name`/`user.email` so `repo.signature()` works regardless of
/// whether the environment running the test has a global git identity configured (CI
/// runners don't, by default).
pub fn set_test_identity(repo: &Repository) {
    let mut config = repo.config().expect("open repo config");
    config.set_str("user.name", "PushGit Test").unwrap();
    config
        .set_str("user.email", "test@pushgit.invalid")
        .unwrap();
}

/// A throwaway repo with a single commit on `main`, cleaned up when `TempDir` drops.
pub fn repo_init() -> (TempDir, Repository) {
    let td = TempDir::new().expect("create temp dir for fixture repo");

    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(td.path(), &opts).expect("init fixture repo");

    {
        set_test_identity(&repo);

        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = repo.signature().unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "initial commit",
            &tree,
            &[],
        )
        .expect("create initial commit");
    }

    (td, repo)
}
