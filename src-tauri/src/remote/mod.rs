// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Remote configuration, and the shell-out layer for clone/fetch/pull/push — subprocess
//! wrapper around system `git`, environment/credential passthrough, stdout/stderr
//! progress-line parsing, cancellation via child-process termination.
//! The environment is inherited unmodified (no credential interception of any kind):
//! whatever `ssh-agent`/credential helper/`~/.gitconfig` the user
//! already has configured is exactly what these commands see.

mod cancellation;
mod progress;
mod version;

pub use cancellation::CancellationRegistry;
pub use progress::{parse_progress_line, RemoteProgress};
pub use version::{check_git_version, GitVersionCheck};

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use git2::BranchType;
use tauri::ipc::Channel;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::branch::{self, MergeOutcome};
use crate::error::{PushGitError, PushGitResult};

/// Forwards at most one update per ~60ms.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(60);

/// Shells out to the real `git` binary for a command with no progress to stream (nothing
/// network-bound) — `pub(crate)` so `graph::commit_graph_file` can reuse it for `git
/// commit-graph write`, another capability `git2`/libgit2-rs doesn't expose at all, rather
/// than duplicating this subprocess wrapper.
pub(crate) async fn run_git(args: &[&str], cwd: Option<&Path>) -> PushGitResult<String> {
    let mut command = Command::new("git");
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let describe = || format!("git {}", args.join(" "));

    let output = command
        .output()
        .await
        .map_err(|e| PushGitError::Subprocess {
            command: describe(),
            message: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(PushGitError::Subprocess {
            command: describe(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Like `run_git`, but for a network-bound command (fetch/push): streams parsed progress
/// through `progress` and checks `cancel` between each chunk of
/// output, killing the child and returning `PushGitError::Cancelled` if it's set. `--progress`
/// must already be in `args` — git suppresses its own progress reporting by default whenever
/// stderr isn't a terminal, which a piped subprocess never is, so without it this would
/// silently never produce anything to parse.
async fn run_git_streaming(
    args: &[&str],
    cwd: Option<&Path>,
    progress: &Channel<RemoteProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let mut command = Command::new("git");
    command
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let describe = || format!("git {}", args.join(" "));

    let mut child = command.spawn().map_err(|e| PushGitError::Subprocess {
        command: describe(),
        message: e.to_string(),
    })?;
    let stderr = child.stderr.take().expect("stderr was piped above");
    let mut reader = BufReader::new(stderr);

    let mut captured_stderr = String::new();
    let mut last_sent: Option<Instant> = None;
    let mut chunk = Vec::new();

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Err(PushGitError::Cancelled);
        }

        chunk.clear();
        let read = read_progress_chunk(&mut reader, &mut chunk)
            .await
            .map_err(|e| PushGitError::Subprocess {
                command: describe(),
                message: e.to_string(),
            })?;
        if read == 0 {
            break;
        }

        let text = String::from_utf8_lossy(&chunk);
        captured_stderr.push_str(&text);

        if let Some(update) = parse_progress_line(&text) {
            let due = last_sent.is_none_or(|t| t.elapsed() >= PROGRESS_THROTTLE);
            if due {
                let _ = progress.send(update);
                last_sent = Some(Instant::now());
            }
        }
    }

    let status = child.wait().await.map_err(|e| PushGitError::Subprocess {
        command: describe(),
        message: e.to_string(),
    })?;

    if !status.success() {
        return Err(PushGitError::Subprocess {
            command: describe(),
            message: captured_stderr.trim().to_string(),
        });
    }

    Ok(())
}

/// Reads into `buf` up to (not including) the next `\n` or `\r` — git's own progress
/// reporting overwrites one terminal line via `\r` between updates rather than emitting a
/// fresh line each time, so splitting on `\n` alone would collapse an entire phase's worth
/// of updates into one unparseable line. Returns the number of bytes consumed (including
/// the delimiter, so `0` unambiguously means EOF).
async fn read_progress_chunk<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize> {
    let mut byte = [0u8; 1];
    let mut consumed = 0;
    loop {
        let n = reader.read(&mut byte).await?;
        if n == 0 {
            return Ok(consumed);
        }
        consumed += 1;
        if byte[0] == b'\n' || byte[0] == b'\r' {
            return Ok(consumed);
        }
        buf.push(byte[0]);
    }
}

/// Fetches `remote_name`, streaming progress and honoring `cancel`.
/// `-c core.symlinks=false` is the documented mitigation for
/// CVE-2024-32002-class submodule-hook attacks — cheap insurance
/// since PushGit's own operations never depend on symlink support during a fetch.
pub async fn fetch(
    repo_path: &Path,
    remote_name: &str,
    progress: &Channel<RemoteProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    run_git_streaming(
        &[
            "-c",
            "core.symlinks=false",
            "fetch",
            "--progress",
            remote_name,
        ],
        Some(repo_path),
        progress,
        cancel,
    )
    .await
}

/// Fetches, then merges the current branch's upstream into HEAD — "pull" as fetch +
/// merge. Rebase-on-pull (`pull.rebase`) is not yet
/// supported; this always merges. Only the fetch half streams progress/honors cancel — the
/// merge itself is a local, non-network git2 operation with nothing to stream and nothing
/// worth cancelling mid-flight.
pub async fn pull(
    repo_path: &Path,
    remote_name: &str,
    progress: &Channel<RemoteProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<MergeOutcome> {
    fetch(repo_path, remote_name, progress, cancel).await?;

    let repo = crate::repo::open(repo_path)?;
    let head_branch_name = repo
        .head()?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| PushGitError::Invalid("detached HEAD; nothing to pull into".to_string()))?;

    let local_branch = repo.find_branch(&head_branch_name, BranchType::Local)?;
    let upstream = local_branch.upstream().map_err(|_| {
        PushGitError::Invalid(format!(
            "branch '{head_branch_name}' has no upstream configured"
        ))
    })?;
    let upstream_name = upstream
        .name()?
        .ok_or_else(|| PushGitError::Invalid("upstream branch has no name".to_string()))?
        .to_string();

    branch::merge_branch(&repo, &upstream_name)
}

/// Pushes `branch_name` to `remote_name`, using a matching refspec (`branch:branch`),
/// streaming progress and honoring `cancel`. Always passes
/// `--follow-tags`, so annotated tags reachable from the pushed commits go along with it —
/// unlike `--tags`, this never pushes unrelated local-only tags.
pub async fn push(
    repo_path: &Path,
    remote_name: &str,
    branch_name: &str,
    force: bool,
    progress: &Channel<RemoteProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let refspec = format!("{branch_name}:{branch_name}");
    let mut args = vec!["push", "--progress", "--follow-tags"];
    if force {
        args.push("--force");
    }
    args.push(remote_name);
    args.push(&refspec);

    run_git_streaming(&args, Some(repo_path), progress, cancel).await
}

/// Clones `url` into `dest`, which must not already exist.
pub async fn clone(url: &str, dest: &Path) -> PushGitResult<()> {
    let dest_str = dest
        .to_str()
        .ok_or_else(|| PushGitError::Invalid("destination path is not valid UTF-8".to_string()))?;

    run_git(&["-c", "core.symlinks=false", "clone", url, dest_str], None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{repo_init, set_test_identity};
    use std::fs;
    use std::path::Path as StdPath;

    fn commit_file(repo: &git2::Repository, name: &str, content: &str) -> git2::Oid {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(StdPath::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &[&head])
            .unwrap()
    }

    /// A local bare repo used as a fake "origin" — no network access.
    fn bare_remote() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        dir
    }

    /// A channel that discards every message and a never-set cancellation token — for tests
    /// that only care about the fetch/push/pull outcome, not the progress stream itself.
    fn no_op_progress() -> (Channel<RemoteProgress>, Arc<AtomicBool>) {
        (Channel::new(|_| Ok(())), Arc::new(AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn clone_creates_a_working_checkout() {
        let (origin_dir, origin_repo) = repo_init();
        commit_file(&origin_repo, "a.txt", "a\n");

        let dest = tempfile::TempDir::new().unwrap();
        let dest_path = dest.path().join("clone");

        clone(origin_dir.path().to_str().unwrap(), &dest_path)
            .await
            .unwrap();

        assert!(dest_path.join("a.txt").exists());
        assert!(dest_path.join(".git").exists());
    }

    #[tokio::test]
    async fn fetch_and_push_round_trip_through_a_bare_remote() {
        let remote_dir = bare_remote();
        let remote_url = remote_dir.path().to_str().unwrap();

        let dest_a = tempfile::TempDir::new().unwrap().keep();
        clone(remote_url, &dest_a).await.unwrap();
        let repo_a = crate::repo::open(&dest_a).unwrap();
        set_test_identity(&repo_a);
        // A fresh clone of an empty bare repo starts with an unborn HEAD; give it a first
        // commit and push it so there's something for the second clone to fetch.
        fs::write(dest_a.join("seed.txt"), "seed\n").unwrap();
        let mut index = repo_a.index().unwrap();
        index.add_path(StdPath::new("seed.txt")).unwrap();
        index.write().unwrap();
        let tree = repo_a.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo_a.signature().unwrap();
        repo_a
            .commit(Some("HEAD"), &sig, &sig, "seed", &tree, &[])
            .unwrap();
        let head_name = repo_a.head().unwrap().shorthand().unwrap().to_string();
        let (progress, cancel) = no_op_progress();
        push(&dest_a, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        let dest_b = tempfile::TempDir::new().unwrap().keep();
        clone(remote_url, &dest_b).await.unwrap();
        assert!(dest_b.join("seed.txt").exists());

        // dest_a pushes a new commit; dest_b should be able to fetch it.
        commit_file(&repo_a, "more.txt", "more\n");
        push(&dest_a, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        fetch(&dest_b, "origin", &progress, &cancel).await.unwrap();
        let repo_b = crate::repo::open(&dest_b).unwrap();
        let remote_head = repo_b
            .find_reference(&format!("refs/remotes/origin/{head_name}"))
            .unwrap()
            .target()
            .unwrap();
        let a_head = repo_a.head().unwrap().target().unwrap();
        assert_eq!(remote_head, a_head);
    }

    #[tokio::test]
    async fn pull_fetches_and_merges_the_upstream() {
        let remote_dir = bare_remote();
        let remote_url = remote_dir.path().to_str().unwrap();

        let dest_a = tempfile::TempDir::new().unwrap().keep();
        let repo_a = git2::Repository::init(&dest_a).unwrap();
        set_test_identity(&repo_a);
        fs::write(dest_a.join("seed.txt"), "seed\n").unwrap();
        let mut index = repo_a.index().unwrap();
        index.add_path(StdPath::new("seed.txt")).unwrap();
        index.write().unwrap();
        let tree = repo_a.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo_a.signature().unwrap();
        repo_a
            .commit(Some("HEAD"), &sig, &sig, "seed", &tree, &[])
            .unwrap();
        let head_name = repo_a.head().unwrap().shorthand().unwrap().to_string();
        repo_a.remote("origin", remote_url).unwrap();
        let (progress, cancel) = no_op_progress();
        push(&dest_a, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        let dest_b = tempfile::TempDir::new().unwrap().keep();
        clone(remote_url, &dest_b).await.unwrap();
        let repo_b = crate::repo::open(&dest_b).unwrap();
        let mut local_branch = repo_b.find_branch(&head_name, BranchType::Local).unwrap();
        local_branch
            .set_upstream(Some(&format!("origin/{head_name}")))
            .unwrap();

        commit_file(&repo_a, "more.txt", "more\n");
        push(&dest_a, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        let outcome = pull(&dest_b, "origin", &progress, &cancel).await.unwrap();

        assert!(matches!(outcome, MergeOutcome::FastForward));
        assert!(dest_b.join("more.txt").exists());
    }

    #[tokio::test]
    async fn push_streams_progress_for_a_real_transfer() {
        let remote_dir = bare_remote();
        let remote_url = remote_dir.path().to_str().unwrap();

        let dest_a = tempfile::TempDir::new().unwrap().keep();
        let repo_a = git2::Repository::init(&dest_a).unwrap();
        set_test_identity(&repo_a);
        let mut index = repo_a.index().unwrap();
        // Enough objects that `git --progress` emits intermediate percentages rather than
        // an instant jump to 100% with nothing in between to parse.
        for i in 0..40 {
            let name = format!("f{i}.txt");
            fs::write(dest_a.join(&name), format!("content {i}\n")).unwrap();
            index.add_path(StdPath::new(&name)).unwrap();
        }
        index.write().unwrap();
        let tree = repo_a.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo_a.signature().unwrap();
        repo_a
            .commit(Some("HEAD"), &sig, &sig, "seed", &tree, &[])
            .unwrap();
        let head_name = repo_a.head().unwrap().shorthand().unwrap().to_string();
        repo_a.remote("origin", remote_url).unwrap();

        let received = Arc::new(std::sync::Mutex::new(0usize));
        let received_for_callback = received.clone();
        let progress = Channel::new(move |_| {
            *received_for_callback.lock().unwrap() += 1;
            Ok(())
        });
        let cancel = Arc::new(AtomicBool::new(false));

        push(&dest_a, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        assert!(
            *received.lock().unwrap() > 0,
            "expected at least one progress update for a 40-file push"
        );
    }

    #[tokio::test]
    async fn push_follows_an_annotated_tag_reachable_from_the_pushed_commit() {
        let remote_dir = bare_remote();
        let remote_url = remote_dir.path().to_str().unwrap();

        let dest = tempfile::TempDir::new().unwrap().keep();
        let repo = git2::Repository::init(&dest).unwrap();
        set_test_identity(&repo);
        fs::write(dest.join("a.txt"), "a\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(StdPath::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "seed", &tree, &[])
            .unwrap();
        let commit_obj = repo.find_object(oid, None).unwrap();
        repo.tag("v1.0.0", &commit_obj, &sig, "release v1.0.0", false)
            .unwrap();
        let head_name = repo.head().unwrap().shorthand().unwrap().to_string();
        repo.remote("origin", remote_url).unwrap();

        let (progress, cancel) = no_op_progress();
        push(&dest, "origin", &head_name, false, &progress, &cancel)
            .await
            .unwrap();

        let remote_repo = git2::Repository::open(remote_dir.path()).unwrap();
        assert!(
            remote_repo.find_reference("refs/tags/v1.0.0").is_ok(),
            "expected --follow-tags to push the annotated tag along with the branch"
        );
    }

    #[tokio::test]
    async fn a_pre_cancelled_token_stops_the_operation_before_it_completes() {
        let remote_dir = bare_remote();
        let remote_url = remote_dir.path().to_str().unwrap();
        let dest = tempfile::TempDir::new().unwrap().keep();
        clone(remote_url, &dest).await.unwrap();

        let (progress, _) = no_op_progress();
        let cancel = Arc::new(AtomicBool::new(true)); // already cancelled before starting

        let result = fetch(&dest, "origin", &progress, &cancel).await;

        assert!(matches!(result, Err(PushGitError::Cancelled)));
    }
}
