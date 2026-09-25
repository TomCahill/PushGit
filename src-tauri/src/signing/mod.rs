// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! GPG/SSH commit signing and verification. libgit2 can inject an already-computed
//! signature (`Repository::commit_signed`) but never produces or checks one itself — real
//! `git` doesn't either, it shells out to `gpg`/`ssh-keygen` (`gpg.program`/
//! `gpg.ssh.program`), so this module does the same rather than reimplementing either
//! signature format. See `.private/feature/gpg-ssh-commit-signing/PLAN.md`.

use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

use git2::{Commit, Oid, Repository, Signature, Tree};
use serde::{Deserialize, Serialize};

use crate::error::{PushGitError, PushGitResult};
use crate::shell_env;

fn invalid(message: impl Into<String>) -> PushGitError {
    PushGitError::Invalid(message.into())
}

const DEFAULT_GPG_PROGRAM: &str = "gpg";
const DEFAULT_SSH_PROGRAM: &str = "ssh-keygen";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignFormat {
    OpenPgp,
    Ssh,
}

impl SignFormat {
    fn from_config_value(value: &str) -> Self {
        if value.eq_ignore_ascii_case("ssh") {
            SignFormat::Ssh
        } else {
            SignFormat::OpenPgp
        }
    }

    fn as_config_value(self) -> &'static str {
        match self {
            SignFormat::OpenPgp => "openpgp",
            SignFormat::Ssh => "ssh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningConfig {
    pub format: SignFormat,
    pub key: Option<String>,
    pub program: String,
}

/// Reads `gpg.format`/`user.signingkey`/`gpg.program`/`gpg.ssh.program` from the repo's
/// merged git config (local + global + system, the same layering `repo.config()` already
/// gives every other reader in this codebase). Missing/blank values fall back to real git's
/// own defaults: `openpgp` format, no key override (gpg picks its own default secret key),
/// `gpg`/`ssh-keygen` as the program.
pub fn resolve_signing_config(repo: &Repository) -> PushGitResult<SigningConfig> {
    resolve_from_config(&repo.config()?)
}

/// Split out from `resolve_signing_config` so tests can exercise the actual defaulting
/// logic against a single isolated config file, instead of `Repository::config()`'s merged
/// local+global+system view — which would otherwise make "nothing configured" tests depend
/// on whatever the machine running them happens to have in its real `~/.gitconfig`.
fn resolve_from_config(config: &git2::Config) -> PushGitResult<SigningConfig> {
    let format = config
        .get_string("gpg.format")
        .map(|value| SignFormat::from_config_value(&value))
        .unwrap_or(SignFormat::OpenPgp);
    let key = config
        .get_string("user.signingkey")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let program_key = match format {
        SignFormat::OpenPgp => "gpg.program",
        SignFormat::Ssh => "gpg.ssh.program",
    };
    let program = config
        .get_string(program_key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            match format {
                SignFormat::OpenPgp => DEFAULT_GPG_PROGRAM,
                SignFormat::Ssh => DEFAULT_SSH_PROGRAM,
            }
            .to_string()
        });
    Ok(SigningConfig {
        format,
        key,
        program,
    })
}

/// Writes `gpg.format`/`user.signingkey`/`commit.gpgsign` (and, if non-empty,
/// `gpg.program`/`gpg.ssh.program`) to the repo's *local* git config — the same
/// `repo.config()?.set_str(...)` pattern `workflow::init_workflow` already uses for
/// `[gitflow ...]` keys. This is real git config: a terminal `git config user.signingkey
/// ...` and this settings panel stay interchangeable, nothing PushGit-specific to migrate.
pub fn set_signing_config(
    repo: &Repository,
    format: SignFormat,
    key: Option<&str>,
    gpg_program: Option<&str>,
    ssh_program: Option<&str>,
    sign_by_default: bool,
) -> PushGitResult<()> {
    let mut config = repo.config()?;
    config.set_str("gpg.format", format.as_config_value())?;
    config.set_bool("commit.gpgsign", sign_by_default)?;
    match key.filter(|value| !value.trim().is_empty()) {
        Some(value) => config.set_str("user.signingkey", value)?,
        None => {
            let _ = config.remove("user.signingkey");
        }
    }
    match gpg_program.filter(|value| !value.trim().is_empty()) {
        Some(value) => config.set_str("gpg.program", value)?,
        None => {
            let _ = config.remove("gpg.program");
        }
    }
    match ssh_program.filter(|value| !value.trim().is_empty()) {
        Some(value) => config.set_str("gpg.ssh.program", value)?,
        None => {
            let _ = config.remove("gpg.ssh.program");
        }
    }
    Ok(())
}

/// Raw config view for the Settings panel's "Commit signing" section — deliberately
/// distinct from `resolve_signing_config`'s "resolved with real defaults filled in" (used
/// by actual signing logic): an unset `gpg.program`/`gpg.ssh.program` shows as blank here
/// rather than the resolved `"gpg"`/`"ssh-keygen"`, so the UI can tell "explicitly set" from
/// "using the default" the same way `external_tools`' resolved-command hint does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningConfigView {
    pub format: SignFormat,
    pub key: Option<String>,
    pub gpg_program: Option<String>,
    pub ssh_program: Option<String>,
    pub sign_by_default: bool,
}

pub fn config_view(repo: &Repository) -> PushGitResult<SigningConfigView> {
    let config = repo.config()?;
    let format = config
        .get_string("gpg.format")
        .map(|value| SignFormat::from_config_value(&value))
        .unwrap_or(SignFormat::OpenPgp);
    let non_empty = |key: &str| -> Option<String> {
        config.get_string(key).ok().filter(|v| !v.trim().is_empty())
    };
    Ok(SigningConfigView {
        format,
        key: non_empty("user.signingkey"),
        gpg_program: non_empty("gpg.program"),
        ssh_program: non_empty("gpg.ssh.program"),
        sign_by_default: config.get_bool("commit.gpgsign").unwrap_or(false),
    })
}

/// `Some(v)` is an explicit per-call override (the commit box's "Sign commit" checkbox);
/// `None` follows `commit.gpgsign` (default `false`), for call sites with no such checkbox
/// (merge, cherry-pick) that simply behave like plain `git merge`/`git cherry-pick` do.
pub fn should_sign(repo: &Repository, override_: Option<bool>) -> PushGitResult<bool> {
    match override_ {
        Some(value) => Ok(value),
        None => should_sign_from_config(&repo.config()?),
    }
}

/// Split out for the same isolation reason as `resolve_from_config`.
fn should_sign_from_config(config: &git2::Config) -> PushGitResult<bool> {
    Ok(config.get_bool("commit.gpgsign").unwrap_or(false))
}

/// Drop-in replacement for `repo.commit(update_ref, ...)` that additionally signs the
/// commit when `should_sign` resolves true. Every real (non-test, non-bookkeeping)
/// commit-creation call site in this codebase should go through this rather than
/// `repo.commit` directly — see the plan doc for the full list and why `undo/`'s manifest
/// commits are the deliberate exception.
#[allow(clippy::too_many_arguments)]
pub fn create_commit(
    repo: &Repository,
    update_ref: Option<&str>,
    author: &Signature<'_>,
    committer: &Signature<'_>,
    message: &str,
    tree: &Tree<'_>,
    parents: &[&Commit<'_>],
    sign: Option<bool>,
) -> PushGitResult<Oid> {
    // Always creates the commit object with `update_ref: None`, then moves the ref itself
    // via `update_reference` (a force-write) — for both the signed and unsigned path alike.
    // `repo.commit(Some(ref), ...)` enforces a "new commit's first parent must equal the
    // ref's current target" safety check; that's correct for an ordinary new commit (whose
    // first parent naturally *is* current HEAD), but `stage::commit`'s amend path passes
    // the *original* commit's own parents, deliberately not current HEAD, to rewrite HEAD
    // in place — exactly what `Commit::amend` used to do without this check. Bypassing it
    // here for every caller keeps signed and unsigned amend behavior identical, and is a
    // no-op for every other caller, whose first parent already matches the ref target.
    let oid = if !should_sign(repo, sign)? {
        repo.commit(None, author, committer, message, tree, parents)?
    } else {
        let signing_config = resolve_signing_config(repo)?;
        let content = repo.commit_create_buffer(author, committer, message, tree, parents)?;
        let content = std::str::from_utf8(&content)
            .map_err(|_| invalid("commit content is not valid UTF-8"))?;
        let signature = sign_buffer(content, &signing_config)?;
        repo.commit_signed(content, &signature, Some("gpgsig"))?
    };
    update_reference(repo, update_ref, oid, message)?;
    Ok(oid)
}

/// `commit_signed` never touches any ref — unlike `repo.commit`, which optionally moves one
/// as part of the same call. For `"HEAD"` specifically, resolves what `repo.commit` itself
/// resolves it to: `find_reference("HEAD")?.symbolic_target()` covers the born
/// (`refs/heads/main`) and unborn (still resolves even though that ref doesn't exist yet)
/// cases, falling back to a direct `"HEAD"` update for a detached HEAD (no symbolic
/// target). Any other literal ref name is updated directly.
fn update_reference(
    repo: &Repository,
    update_ref: Option<&str>,
    oid: Oid,
    message: &str,
) -> PushGitResult<()> {
    let Some(name) = update_ref else {
        return Ok(());
    };
    let log_message = format!("commit (signed): {message}");
    let target = if name == "HEAD" {
        repo.find_reference("HEAD")
            .ok()
            .and_then(|head| head.symbolic_target().map(str::to_string))
            .unwrap_or_else(|| "HEAD".to_string())
    } else {
        name.to_string()
    };
    repo.reference(&target, oid, true, &log_message)?;
    Ok(())
}

fn sign_buffer(content: &str, config: &SigningConfig) -> PushGitResult<String> {
    match config.format {
        SignFormat::OpenPgp => sign_openpgp(content, config),
        SignFormat::Ssh => sign_ssh(content, config),
    }
}

/// `--status-fd=2` mirrors real git's own invocation so a failure's stderr carries gpg's
/// machine-readable status alongside the human message. `-u <key>` is omitted when
/// `user.signingkey` is unset, matching real git's fallback to gpg's own default secret
/// key. Content and signature are both always small (a commit header + message), so
/// sequential write-then-read (no separate reader thread) can't deadlock on pipe buffers
/// the way it could for arbitrary large data.
fn sign_openpgp(content: &str, config: &SigningConfig) -> PushGitResult<String> {
    let mut command = shell_env::command(&config.program);
    command
        .arg("--status-fd=2")
        .arg("-bsa")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(key) = &config.key {
        command.arg("-u").arg(key);
    }

    let mut child = command.spawn().map_err(|e| PushGitError::Subprocess {
        command: config.program.clone(),
        message: e.to_string(),
    })?;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(content.as_bytes())?;
    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: config.program.clone(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| invalid("gpg produced a non-UTF-8 signature"))
}

/// Real git's own SSH-signing convention: write the content to a file, run `ssh-keygen -Y
/// sign -n git -f <key> <file>`, then read back `<file>.sig`. `user.signingkey` is a path to
/// a private *or* public key file, matching real git — an agent-only setup (public key on
/// disk, private key only in `ssh-agent`) works unmodified since `ssh-keygen -Y sign`
/// already falls back to the agent itself, and this subprocess inherits `SSH_AUTH_SOCK` like
/// every other subprocess in this codebase.
fn sign_ssh(content: &str, config: &SigningConfig) -> PushGitResult<String> {
    let key = config.key.as_deref().ok_or_else(|| {
        invalid("user.signingkey must be set to a key file path for SSH commit signing")
    })?;

    let temp = tempfile::Builder::new()
        .prefix("pushgit-sign-")
        .tempfile()?;
    std::fs::write(temp.path(), content)?;

    let output = shell_env::command(&config.program)
        .args(["-Y", "sign", "-n", "git", "-f", key])
        .arg(temp.path())
        .output()
        .map_err(|e| PushGitError::Subprocess {
            command: config.program.clone(),
            message: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: config.program.clone(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let sig_path = PathBuf::from(format!("{}.sig", temp.path().display()));
    let signature = std::fs::read_to_string(&sig_path)?;
    let _ = std::fs::remove_file(&sig_path);
    Ok(signature)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum VerificationStatus {
    Unsigned,
    Good { signer: String },
    Bad,
    UnknownKey,
    NoAllowedSigners,
    Error { message: String },
}

/// A commit with no `gpgsig` header at all short-circuits to `Unsigned` with no subprocess
/// spawned — the overwhelmingly common case, and the reason the graph can check
/// `has_signature` on every row for free while only this function's callers pay for a real
/// subprocess, and only for commits that actually carry a signature. Shells to real `git
/// verify-commit`, not `gpg`/`ssh-keygen` directly, since git already knows how to dispatch
/// between the two signature formats per-commit and, for SSH, honor
/// `gpg.ssh.allowedSignersFile` — duplicating that branching here would just be a second,
/// divergence-prone copy of logic real git already gets right.
pub fn verify_commit(repo: &Repository, oid: &str) -> PushGitResult<VerificationStatus> {
    let commit = repo.find_commit(repo.revparse_single(oid)?.id())?;
    if commit.header_field_bytes("gpgsig").is_err() {
        return Ok(VerificationStatus::Unsigned);
    }

    let workdir = repo
        .workdir()
        .ok_or_else(|| invalid("repository has no working directory"))?;
    let output = shell_env::command("git")
        .args(["verify-commit", "--raw", oid])
        .current_dir(workdir)
        .output()?;

    Ok(parse_verify_output(&String::from_utf8_lossy(
        &output.stderr,
    )))
}

/// `git verify-commit --raw` prints the same GPG status-protocol tokens for both signature
/// formats (git normalizes SSH verification onto this vocabulary too), so one parser covers
/// both. Anything not recognized collapses to `Error` with the raw stderr rather than a
/// guessed status — exact wording is pinned down against real `gpg`/`ssh-keygen`/`git
/// verify-commit` output in this module's fixture tests, not assumed.
fn parse_verify_output(stderr: &str) -> VerificationStatus {
    if stderr.contains("allowedSignersFile") || stderr.contains("No principal matched") {
        return VerificationStatus::NoAllowedSigners;
    }
    if let Some(line) = stderr
        .lines()
        .find(|line| line.contains("GOODSIG") || line.contains("VALIDSIG"))
    {
        let signer = line
            .split_once("GOODSIG")
            .or_else(|| line.split_once("VALIDSIG"))
            .map(|(_, rest)| rest.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown signer")
            .to_string();
        return VerificationStatus::Good { signer };
    }
    if stderr.contains("NO_PUBKEY") || stderr.contains("no public key") {
        return VerificationStatus::UnknownKey;
    }
    if stderr.contains("BADSIG") {
        return VerificationStatus::Bad;
    }
    VerificationStatus::Error {
        message: stderr.trim().to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpgSecretKey {
    pub key_id: String,
    pub user_id: String,
}

/// Backs the Settings panel's "Detect GPG keys" button (OpenPGP format only — there's no
/// equivalent "list keys" primitive for SSH signing keys, which get a file picker instead).
/// Always shells to the default `gpg`, not a configured `gpg.program` override: this lists
/// keys to help the user *fill in* `user.signingkey`, before any repo-specific program
/// override necessarily applies.
pub fn list_gpg_secret_keys() -> PushGitResult<Vec<GpgSecretKey>> {
    let output = shell_env::command(DEFAULT_GPG_PROGRAM)
        .args(["--list-secret-keys", "--with-colons"])
        .output()
        .map_err(|e| PushGitError::Subprocess {
            command: DEFAULT_GPG_PROGRAM.to_string(),
            message: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: DEFAULT_GPG_PROGRAM.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(parse_gpg_secret_keys(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// One row per secret key, using its first `uid` record — see gpg's own `DETAILS` doc for
/// the colon-record format. `sec` starts a new key block; the `fpr` record immediately
/// following it is that key's fingerprint; the first `uid` record after that pairs the two
/// into one result, ignoring any additional uids on the same key.
fn parse_gpg_secret_keys(stdout: &str) -> Vec<GpgSecretKey> {
    let mut keys = Vec::new();
    let mut current_fpr: Option<String> = None;
    let mut recorded = false;
    for line in stdout.lines() {
        let mut fields = line.split(':');
        match fields.next() {
            Some("sec") => {
                current_fpr = None;
                recorded = false;
            }
            Some("fpr") if current_fpr.is_none() => {
                current_fpr = fields.nth(8).map(str::to_string).filter(|v| !v.is_empty());
            }
            Some("uid") if !recorded => {
                if let (Some(fpr), Some(user_id)) =
                    (&current_fpr, fields.nth(8).filter(|v| !v.is_empty()))
                {
                    keys.push(GpgSecretKey {
                        key_id: fpr.clone(),
                        user_id: user_id.to_string(),
                    });
                    recorded = true;
                }
            }
            _ => {}
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        gen_gpg_key, gen_ssh_keypair, gnupg_home, repo_init, with_gnupg_home,
    };
    use std::fs;

    /// A single-file, single-level `Config` with no local/global/system merging at all —
    /// unlike `Repository::config()`, this can't pick up whatever the machine actually
    /// running the tests has in its real `~/.gitconfig`, which on a developer machine that
    /// itself has commit signing configured (as this one does) would otherwise make
    /// "nothing configured" tests fail depending on who runs them.
    fn isolated_config(dir: &std::path::Path) -> git2::Config {
        let path = dir.join("isolated.gitconfig");
        fs::write(&path, "").unwrap();
        git2::Config::open(&path).unwrap()
    }

    #[test]
    fn resolve_signing_config_defaults_to_openpgp_with_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = resolve_from_config(&isolated_config(dir.path())).unwrap();
        assert_eq!(config.format, SignFormat::OpenPgp);
        assert_eq!(config.key, None);
        assert_eq!(config.program, "gpg");
    }

    #[test]
    fn resolve_signing_config_reads_ssh_format_and_key() {
        let (_dir, repo) = repo_init();
        let mut config = repo.config().unwrap();
        config.set_str("gpg.format", "ssh").unwrap();
        config
            .set_str("user.signingkey", "/home/me/.ssh/id_ed25519")
            .unwrap();

        let resolved = resolve_signing_config(&repo).unwrap();
        assert_eq!(resolved.format, SignFormat::Ssh);
        assert_eq!(resolved.key.as_deref(), Some("/home/me/.ssh/id_ed25519"));
        assert_eq!(resolved.program, "ssh-keygen");
    }

    #[test]
    fn should_sign_override_wins_over_config() {
        let (_dir, repo) = repo_init();
        repo.config()
            .unwrap()
            .set_bool("commit.gpgsign", true)
            .unwrap();
        assert!(!should_sign(&repo, Some(false)).unwrap());
        assert!(should_sign(&repo, None).unwrap());
        assert!(should_sign(&repo, Some(true)).unwrap());
    }

    #[test]
    fn should_sign_defaults_false_when_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!should_sign_from_config(&isolated_config(dir.path())).unwrap());
    }

    #[test]
    fn set_signing_config_round_trips_through_resolve() {
        let (_dir, repo) = repo_init();
        set_signing_config(
            &repo,
            SignFormat::Ssh,
            Some("/home/me/.ssh/id_ed25519"),
            None,
            Some("/usr/bin/ssh-keygen"),
            true,
        )
        .unwrap();

        let resolved = resolve_signing_config(&repo).unwrap();
        assert_eq!(resolved.format, SignFormat::Ssh);
        assert_eq!(resolved.key.as_deref(), Some("/home/me/.ssh/id_ed25519"));
        assert_eq!(resolved.program, "/usr/bin/ssh-keygen");
        assert!(should_sign(&repo, None).unwrap());
    }

    #[test]
    fn sign_and_verify_openpgp_round_trip() {
        let gnupghome = gnupg_home();
        let (_dir, repo) = repo_init();
        // A local `user.signingkey` deliberately overrides whatever the machine actually
        // running this test has configured globally (this dev machine has a real one).
        let key = with_gnupg_home(gnupghome.path(), || gen_gpg_key(gnupghome.path()));
        repo.config()
            .unwrap()
            .set_str("user.signingkey", &key)
            .unwrap();

        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let oid = with_gnupg_home(gnupghome.path(), || {
            create_commit(
                &repo,
                Some("HEAD"),
                &sig,
                &sig,
                "signed commit",
                &tree,
                &[&head],
                Some(true),
            )
        })
        .unwrap();

        assert!(repo
            .find_commit(oid)
            .unwrap()
            .header_field_bytes("gpgsig")
            .is_ok());

        let status =
            with_gnupg_home(gnupghome.path(), || verify_commit(&repo, &oid.to_string())).unwrap();
        assert!(
            matches!(status, VerificationStatus::Good { .. }),
            "{status:?}"
        );
    }

    #[test]
    fn verify_commit_reports_unsigned_with_no_subprocess() {
        let (_dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let status = verify_commit(&repo, &head.id().to_string()).unwrap();
        assert_eq!(status, VerificationStatus::Unsigned);
    }

    #[test]
    fn verify_commit_reports_bad_for_a_tampered_signed_commit() {
        let gnupghome = gnupg_home();
        let (_dir, repo) = repo_init();
        let key = with_gnupg_home(gnupghome.path(), || gen_gpg_key(gnupghome.path()));

        let signing_config = SigningConfig {
            format: SignFormat::OpenPgp,
            key: Some(key),
            program: "gpg".to_string(),
        };
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        // Sign the real commit content, but write the signature onto a *different* message
        // than what was actually signed — a deterministic, on-disk "signature doesn't match
        // content" fixture. (`Commit::amend` was tried first and rejected: libgit2 doesn't
        // carry the `gpgsig` header over into the amended commit at all, so that path
        // produces an `Unsigned` commit, not a tampered-but-still-signed one.)
        let real_content = repo
            .commit_create_buffer(&sig, &sig, "original message", &tree, &[&head])
            .unwrap();
        let real_content = std::str::from_utf8(&real_content).unwrap();
        let signature = with_gnupg_home(gnupghome.path(), || {
            sign_buffer(real_content, &signing_config)
        })
        .unwrap();
        let tampered_content = repo
            .commit_create_buffer(&sig, &sig, "tampered message", &tree, &[&head])
            .unwrap();
        let tampered_content = std::str::from_utf8(&tampered_content).unwrap();
        let oid = repo
            .commit_signed(tampered_content, &signature, Some("gpgsig"))
            .unwrap();

        let status =
            with_gnupg_home(gnupghome.path(), || verify_commit(&repo, &oid.to_string())).unwrap();
        assert_eq!(status, VerificationStatus::Bad);
    }

    #[test]
    fn sign_ssh_and_verify_reports_no_allowed_signers_when_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = gen_ssh_keypair(dir.path());
        let (_repo_dir, repo) = repo_init();
        repo.config().unwrap().set_str("gpg.format", "ssh").unwrap();
        repo.config()
            .unwrap()
            .set_str("user.signingkey", key_path.to_str().unwrap())
            .unwrap();

        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let oid = create_commit(
            &repo,
            Some("HEAD"),
            &sig,
            &sig,
            "ssh signed commit",
            &tree,
            &[&head],
            Some(true),
        )
        .unwrap();

        assert!(repo
            .find_commit(oid)
            .unwrap()
            .header_field_bytes("gpgsig")
            .is_ok());

        // No `gpg.ssh.allowedSignersFile` configured — git can't map the key to a trusted
        // identity, so this must not silently read as `Good`.
        let status = verify_commit(&repo, &oid.to_string()).unwrap();
        assert_eq!(status, VerificationStatus::NoAllowedSigners);
    }

    #[test]
    fn create_commit_updates_a_detached_head_directly() {
        let (_dir, repo) = repo_init();
        let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
        repo.set_head_detached(head_oid).unwrap();

        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let oid = create_commit(
            &repo,
            Some("HEAD"),
            &sig,
            &sig,
            "detached commit",
            &tree,
            &[&head],
            Some(false),
        )
        .unwrap();

        assert_eq!(repo.head().unwrap().target(), Some(oid));
        assert!(repo.head_detached().unwrap());
    }

    #[test]
    fn list_gpg_secret_keys_parses_real_gpg_output() {
        let gnupghome = gnupg_home();
        let fpr = with_gnupg_home(gnupghome.path(), || gen_gpg_key(gnupghome.path()));

        let output = with_gnupg_home(gnupghome.path(), || {
            shell_env::command("gpg")
                .args(["--list-secret-keys", "--with-colons"])
                .output()
                .unwrap()
        });
        let keys = parse_gpg_secret_keys(&String::from_utf8_lossy(&output.stdout));

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_id, fpr);
        assert!(keys[0].user_id.contains("PushGit Test"));
    }

    #[test]
    fn parse_gpg_secret_keys_ignores_a_key_with_no_uid() {
        // A key block with `sec`/`fpr` but no `uid` record (e.g. a revoked/expired
        // identity) shouldn't produce a half-populated row.
        let stdout = "sec:::::::::\nfpr:::::::::AAAABBBBCCCCDDDD:\n";
        assert!(parse_gpg_secret_keys(stdout).is_empty());
    }
}
