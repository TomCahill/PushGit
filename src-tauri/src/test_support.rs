// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fixture repositories built as code, not checked in as binary `.git` blobs.
//! Test-only — not compiled into the shipped binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use git2::{Repository, RepositoryInitOptions};
use tempfile::TempDir;

/// Sets repo-local `user.name`/`user.email` so `repo.signature()` works regardless of
/// whether the environment running the test has a global git identity configured (CI
/// runners don't, by default). Also forces `commit.gpgsign` off locally: `signing::
/// create_commit`'s `None` (unset) case reads this from the repo's *merged* config, so
/// without an explicit local override every merge/cherry-pick test would otherwise inherit
/// whatever the machine actually running the tests has configured globally — a real commit
/// signing setup on a dev machine (not hypothetical: this repo's own maintainer has one)
/// would make ordinary fixture tests try to shell out to the real `gpg`/`ssh-keygen` and
/// sign with the developer's own key.
pub fn set_test_identity(repo: &Repository) {
    let mut config = repo.config().expect("open repo config");
    config.set_str("user.name", "PushGit Test").unwrap();
    config
        .set_str("user.email", "test@pushgit.invalid")
        .unwrap();
    config.set_bool("commit.gpgsign", false).unwrap();
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

/// Guards every test that mutates a process-wide env var (`GNUPGHOME`) — `cargo test` runs
/// tests in parallel threads within one process by default, and env vars are process, not
/// thread, state. Shared across every module's signing-related tests so they all serialize
/// against the same lock, not one copy per module.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

pub fn gnupg_home() -> TempDir {
    tempfile::Builder::new()
        .prefix("pushgit-gnupghome-")
        .tempdir()
        .unwrap()
}

/// Generates a throwaway OpenPGP key in an isolated `GNUPGHOME` (never the real user's
/// keyring/agent), returning its key id.
pub fn gen_gpg_key(gnupghome: &Path) -> String {
    let batch = gnupghome.join("batch");
    std::fs::write(
        &batch,
        "Key-Type: eddsa\nKey-Curve: ed25519\nName-Real: PushGit Test\nName-Email: test@pushgit.invalid\nExpire-Date: 0\n%no-protection\n%commit\n",
    )
    .unwrap();
    let status = Command::new("gpg")
        .env("GNUPGHOME", gnupghome)
        .args(["--batch", "--gen-key"])
        .arg(&batch)
        .status()
        .unwrap();
    assert!(status.success(), "gpg --gen-key failed");

    let output = Command::new("gpg")
        .env("GNUPGHOME", gnupghome)
        .args(["--list-secret-keys", "--with-colons"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find(|line| line.starts_with("fpr:"))
        .and_then(|line| line.split(':').nth(9))
        .expect("a generated key id")
        .to_string()
}

/// Runs `f` with `GNUPGHOME` pointed at an isolated directory for the duration of the call —
/// signing/verification shell out to `gpg` inheriting the process environment, so this is
/// what keeps a test off the real user's keyring/agent. Guarded by `ENV_LOCK` since env vars
/// are process-wide, not per-thread.
pub fn with_gnupg_home<T>(gnupghome: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os("GNUPGHOME");
    std::env::set_var("GNUPGHOME", gnupghome);
    let result = f();
    match previous {
        Some(value) => std::env::set_var("GNUPGHOME", value),
        None => std::env::remove_var("GNUPGHOME"),
    }
    result
}

/// Generates a throwaway ed25519 SSH keypair at `<dir>/id_test`, returning the private key
/// path (also usable as `user.signingkey`, matching real git's own convention).
pub fn gen_ssh_keypair(dir: &Path) -> PathBuf {
    let key_path = dir.join("id_test");
    let status = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-f"])
        .arg(&key_path)
        .status()
        .unwrap();
    assert!(status.success(), "ssh-keygen -t ed25519 failed");
    key_path
}
