// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs the standard commit-path git hooks (`pre-commit`, `commit-msg`, `post-commit`)
//! around `stage::commit` — new shell-out surface, not something `git2`/libgit2 does
//! implicitly (libgit2 never executes hooks at all). This exists to
//! give PushGit's commit flow the same default behavior real `git commit` already has (most
//! hook installs, e.g. husky/pre-commit-framework/commitlint, assume every commit runs
//! them), with the `skip_hooks` escape hatch matching plain `git commit --no-verify`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use git2::Repository;

use crate::error::{PushGitError, PushGitResult};

pub mod output;
pub use output::{HookOutputLine, OutputStream};

/// A hook that ran and rejected the commit (non-zero exit) — its combined stdout+stderr
/// becomes the shown message, matching what a terminal `git commit` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRejection {
    pub hook: String,
    pub output: String,
}

impl From<HookRejection> for PushGitError {
    fn from(rejection: HookRejection) -> Self {
        let output = if rejection.output.is_empty() {
            "(no output)".to_string()
        } else {
            rejection.output
        };
        PushGitError::Invalid(format!(
            "`{}` hook rejected the commit:\n{output}",
            rejection.hook
        ))
    }
}

/// Real git resolves hooks via `core.hooksPath` (relative paths are relative to the working
/// directory) if set, else `$GIT_DIR/hooks` — matched here via `Config::get_path` for the
/// same tilde-expansion behavior `stage::commit_message_template` already relies on for
/// `commit.template`.
fn hooks_dir(repo: &Repository) -> PathBuf {
    if let Ok(config) = repo.config() {
        if let Ok(configured) = config.get_path("core.hooksPath") {
            if configured.is_absolute() {
                return configured;
            }
            let workdir = repo.workdir().unwrap_or_else(|| repo.path());
            return workdir.join(configured);
        }
    }
    repo.path().join("hooks")
}

/// Whether `path` is a regular file with at least one executable bit set — git silently
/// ignores a hook file that exists but isn't executable (e.g. the `.sample` templates every
/// `git init` writes, which are never executable), and so does this. Linux-first;
/// a Windows/macOS port will need its own check here.
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn hook_path(repo: &Repository, name: &str) -> Option<PathBuf> {
    let path = hooks_dir(repo).join(name);
    is_executable(&path).then_some(path)
}

/// Runs a hook executable with `args`, cwd'd at the working directory and `GIT_DIR` pointed
/// at the real `.git` (matching what real git sets for hook subprocesses) so a hook that
/// shells back out to `git` itself sees the right repository, forwarding each output line to
/// `on_line` as it's produced (see `output::stream_command`). `Ok(None)` on success,
/// `Ok(Some(rejection))` on a non-zero exit — a launch failure (hook removed mid-race, not
/// actually executable despite the permission check, etc.) is the one case that's a real
/// `PushGitError::Subprocess`, not a rejection.
fn run(
    path: &Path,
    args: &[&str],
    repo: &Repository,
    on_line: &mut dyn FnMut(HookOutputLine),
) -> PushGitResult<Option<HookRejection>> {
    let workdir = repo.workdir().unwrap_or_else(|| repo.path());
    let hook_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut command = Command::new(path);
    command
        .args(args)
        .current_dir(workdir)
        .env("GIT_DIR", repo.path())
        .stdin(Stdio::null());

    let (status, combined) = output::stream_command(command, &hook_name, on_line).map_err(|e| {
        PushGitError::Subprocess {
            command: path.display().to_string(),
            message: e.to_string(),
        }
    })?;

    if status.success() {
        return Ok(None);
    }

    Ok(Some(HookRejection {
        hook: hook_name,
        output: combined.trim().to_string(),
    }))
}

/// Runs `pre-commit` if present and executable. Takes no arguments, matching real git for an
/// ordinary (non-merge, non-amend-only-message) commit — this GUI has no "amend just the
/// message, nothing staged changed" fast path that would need `--amend` passed through.
pub fn run_pre_commit(
    repo: &Repository,
    on_line: &mut dyn FnMut(HookOutputLine),
) -> PushGitResult<Option<HookRejection>> {
    let Some(path) = hook_path(repo, "pre-commit") else {
        return Ok(None);
    };
    run(&path, &[], repo, on_line)
}

/// Runs `commit-msg` if present and executable, against a temp file seeded with `message` —
/// the same on-disk-file contract real git uses so a hook can rewrite the message in place
/// (e.g. appending a `Signed-off-by` trailer). Returns the (possibly hook-rewritten) message
/// unchanged when no hook is configured.
pub fn run_commit_msg(
    repo: &Repository,
    message: &str,
    on_line: &mut dyn FnMut(HookOutputLine),
) -> PushGitResult<Result<String, HookRejection>> {
    let Some(path) = hook_path(repo, "commit-msg") else {
        return Ok(Ok(message.to_string()));
    };

    let msg_path = repo.path().join("COMMIT_EDITMSG");
    std::fs::write(&msg_path, message)?;

    let arg = msg_path.to_string_lossy().into_owned();
    Ok(match run(&path, &[&arg], repo, on_line)? {
        Some(rejection) => Err(rejection),
        None => Ok(std::fs::read_to_string(&msg_path)?),
    })
}

/// Runs `post-commit` if present — purely informational, matching real git exactly: it runs
/// unconditionally (even when `skip_hooks` bypassed `pre-commit`/`commit-msg` — `--no-verify`
/// only ever bypasses those two), and neither its exit code nor its output can affect
/// anything, so any failure here is swallowed rather than propagated.
pub fn run_post_commit(repo: &Repository, on_line: &mut dyn FnMut(HookOutputLine)) {
    if let Some(path) = hook_path(repo, "post-commit") {
        let _ = run(&path, &[], repo, on_line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write_hook(repo: &Repository, name: &str, script: &str) -> PathBuf {
        let dir = repo.path().join("hooks");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn run_pre_commit_is_a_no_op_when_no_hook_is_installed() {
        let (_dir, repo) = repo_init();
        assert!(run_pre_commit(&repo, &mut |_| {}).unwrap().is_none());
    }

    #[test]
    fn run_pre_commit_ignores_a_non_executable_hook_file() {
        let (_dir, repo) = repo_init();
        let dir = repo.path().join("hooks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("pre-commit"), "#!/bin/sh\nexit 1\n").unwrap();
        // Left at the default (non-executable) mode a plain `fs::write` produces.

        assert!(run_pre_commit(&repo, &mut |_| {}).unwrap().is_none());
    }

    #[test]
    fn run_pre_commit_passes_when_the_hook_exits_zero() {
        let (_dir, repo) = repo_init();
        write_hook(&repo, "pre-commit", "#!/bin/sh\nexit 0\n");

        assert!(run_pre_commit(&repo, &mut |_| {}).unwrap().is_none());
    }

    #[test]
    fn run_pre_commit_rejects_with_the_hook_s_output_on_a_non_zero_exit() {
        let (_dir, repo) = repo_init();
        write_hook(
            &repo,
            "pre-commit",
            "#!/bin/sh\necho 'lint failed' >&2\nexit 1\n",
        );

        let rejection = run_pre_commit(&repo, &mut |_| {}).unwrap().unwrap();
        assert_eq!(rejection.hook, "pre-commit");
        assert_eq!(rejection.output, "lint failed");
    }

    #[test]
    fn run_pre_commit_forwards_output_lines_as_they_re_produced() {
        let (_dir, repo) = repo_init();
        write_hook(
            &repo,
            "pre-commit",
            "#!/bin/sh\necho 'checking style' >&2\nexit 0\n",
        );

        let mut lines = Vec::new();
        run_pre_commit(&repo, &mut |line| lines.push(line)).unwrap();

        assert_eq!(
            lines,
            vec![HookOutputLine {
                hook: "pre-commit".to_string(),
                stream: OutputStream::Stderr,
                text: "checking style".to_string(),
            }]
        );
    }

    #[test]
    fn run_commit_msg_returns_the_message_unchanged_when_no_hook_is_installed() {
        let (_dir, repo) = repo_init();
        assert_eq!(
            run_commit_msg(&repo, "original message", &mut |_| {}).unwrap(),
            Ok("original message".to_string())
        );
    }

    #[test]
    fn run_commit_msg_honors_a_hook_that_rewrites_the_file() {
        let (_dir, repo) = repo_init();
        write_hook(
            &repo,
            "commit-msg",
            "#!/bin/sh\necho 'rewritten by hook' > \"$1\"\nexit 0\n",
        );

        assert_eq!(
            run_commit_msg(&repo, "original message", &mut |_| {}).unwrap(),
            Ok("rewritten by hook\n".to_string())
        );
    }

    #[test]
    fn run_commit_msg_rejects_on_a_non_zero_exit() {
        let (_dir, repo) = repo_init();
        write_hook(
            &repo,
            "commit-msg",
            "#!/bin/sh\necho 'bad message format' >&2\nexit 1\n",
        );

        let result = run_commit_msg(&repo, "bad", &mut |_| {}).unwrap();
        let rejection = result.unwrap_err();
        assert_eq!(rejection.hook, "commit-msg");
        assert_eq!(rejection.output, "bad message format");
    }

    #[test]
    fn run_post_commit_swallows_a_failing_hook_rather_than_erroring() {
        let (_dir, repo) = repo_init();
        write_hook(&repo, "post-commit", "#!/bin/sh\nexit 1\n");

        run_post_commit(&repo, &mut |_| {}); // must not panic
    }

    #[test]
    fn hooks_dir_honors_core_hooks_path() {
        let (dir, repo) = repo_init();
        let custom = dir.path().join("custom-hooks");
        fs::create_dir_all(&custom).unwrap();
        repo.config()
            .unwrap()
            .set_str("core.hooksPath", custom.to_str().unwrap())
            .unwrap();
        let hook_path = custom.join("pre-commit");
        fs::write(
            &hook_path,
            "#!/bin/sh\necho 'from custom dir' >&2\nexit 1\n",
        )
        .unwrap();
        let mut perms = fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook_path, perms).unwrap();

        let rejection = run_pre_commit(&repo, &mut |_| {}).unwrap().unwrap();
        assert_eq!(rejection.output, "from custom dir");
    }
}
