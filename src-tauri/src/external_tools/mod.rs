// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shells out to a user-configured external diff/merge tool (Beyond Compare, KDiff3,
//! P4Merge, etc.), reusing real git's own `difftool.<tool>.cmd`/`mergetool.<tool>.cmd`
//! config convention rather than inventing a new one — see
//! `.private/feature/external-diff-merge-tools/PLAN.md` for the full design.

use std::path::{Path, PathBuf};

use git2::Repository;
use serde::{Deserialize, Serialize};

use crate::branch;
use crate::config::AppConfig;
use crate::error::{PushGitError, PushGitResult};
use crate::shell_env;

fn invalid(message: impl Into<String>) -> PushGitError {
    PushGitError::Invalid(message.into())
}

/// Identifies where one side of an external diff invocation gets its content from. Sent by
/// the frontend, which already knows — from whichever view launched this — whether it means
/// the live working directory, the index, or a specific commit-ish.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DiffSide {
    Empty,
    Workdir,
    Index,
    /// `rev` is anything `Repository::revparse_single` accepts (branch, tag, short SHA,
    /// `HEAD~3`, ...), mirroring `diff_between_commits`'s existing param shape.
    Commit {
        rev: String,
    },
}

/// Resolves the command to run for "open in external diff tool": `AppConfig`'s own override
/// if set and non-blank, else this repo's `diff.tool` → `difftool.<tool>.cmd` git config,
/// else `None`.
pub fn resolve_diff_command(repo: &Repository, app_config: &AppConfig) -> Option<String> {
    resolve_command(
        repo,
        app_config.external_tools.diff_command.as_deref(),
        "diff.tool",
        "difftool",
    )
}

/// Same resolution order as `resolve_diff_command`, for `merge.tool`/`mergetool.<tool>.cmd`.
pub fn resolve_merge_command(repo: &Repository, app_config: &AppConfig) -> Option<String> {
    resolve_command(
        repo,
        app_config.external_tools.merge_command.as_deref(),
        "merge.tool",
        "mergetool",
    )
}

fn resolve_command(
    repo: &Repository,
    override_cmd: Option<&str>,
    tool_key: &str,
    section: &str,
) -> Option<String> {
    if let Some(cmd) = override_cmd {
        if !cmd.trim().is_empty() {
            return Some(cmd.to_string());
        }
    }
    resolve_tool_command(repo, tool_key, section)
}

/// Reads `<tool_key>` (`diff.tool`/`merge.tool`) then `<section>.<tool>.cmd`
/// (`difftool.<tool>.cmd`/`mergetool.<tool>.cmd`) from the repo's own git config. Only an
/// explicit `cmd` string is honored — a tool name with no matching `.cmd` entry (relying on
/// one of real git's ~30 built-in tool presets) resolves to `None` the same as no
/// configuration at all; see the plan's Non-goals for why that table isn't replicated here.
fn resolve_tool_command(repo: &Repository, tool_key: &str, section: &str) -> Option<String> {
    let config = repo.config().ok()?;
    let tool = config.get_string(tool_key).ok()?;
    if tool.trim().is_empty() {
        return None;
    }
    config.get_string(&format!("{section}.{tool}.cmd")).ok()
}

/// Resolves one side's raw content. An absent file (the empty side of an add/delete, a path
/// missing from a given tree/index) resolves to an empty buffer rather than an error, matching
/// git's own difftool convention of pointing the tool at an empty temp file for that side.
pub fn resolve_side_bytes(
    repo: &Repository,
    side: &DiffSide,
    path: &str,
) -> PushGitResult<Vec<u8>> {
    match side {
        DiffSide::Empty => Ok(Vec::new()),
        DiffSide::Workdir => {
            let workdir = repo
                .workdir()
                .ok_or_else(|| invalid("repository has no working directory"))?;
            Ok(std::fs::read(workdir.join(path)).unwrap_or_default())
        }
        DiffSide::Index => {
            let index = repo.index()?;
            let Some(entry) = index.get_path(Path::new(path), 0) else {
                return Ok(Vec::new());
            };
            Ok(repo.find_blob(entry.id)?.content().to_vec())
        }
        DiffSide::Commit { rev } => {
            let tree = repo.revparse_single(rev)?.peel_to_commit()?.tree()?;
            let Ok(entry) = tree.get_path(Path::new(path)) else {
                return Ok(Vec::new());
            };
            let blob = entry.to_object(repo)?.peel_to_blob()?;
            Ok(blob.content().to_vec())
        }
    }
}

/// Writes `bytes` under `<temp_dir>/<side>/<basename-of-path>`. `Path::file_name()` strips any
/// directory components (or `..`) `path` might contain, since it's repo-derived — the temp
/// file always lands directly inside `<temp_dir>/<side>/` regardless of what `path` looks like.
/// Keeping the original basename (rather than a generic name) means a tool that
/// syntax-highlights by extension still sees the right one.
fn write_side(
    temp_dir: &tempfile::TempDir,
    side: &str,
    path: &str,
    bytes: &[u8],
) -> PushGitResult<PathBuf> {
    let basename = Path::new(path)
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("file"));
    let dir = temp_dir.path().join(side);
    std::fs::create_dir_all(&dir)?;
    let file_path = dir.join(basename);
    std::fs::write(&file_path, bytes)?;
    Ok(file_path)
}

/// Spawns `sh -c "$cmd"` with `env` set as literal process environment variables — the command
/// string is always locally-trusted (repo or app config, never repo content); only the *paths*
/// substituted via `env` are repo-derived, and those are always PushGit-generated temp paths,
/// never a raw string built by concatenating repo content into the command itself. Runs with
/// the repo's working directory as `cwd`, matching real git's own difftool/mergetool
/// invocation, and blocks until the external tool exits (an interactive GUI diff/merge tool
/// can stay open indefinitely).
///
/// Synchronous, like the rest of this module — called from inside a `tokio::task::spawn_blocking`
/// closure by the `commands::open_external_*` wrappers, the same discipline `commands::commit`
/// already follows for its own slow/arbitrary subprocess (a commit hook): a `git2::Repository`
/// is not `Send`, so it can never be held across an `.await` point — reusing the
/// blocking-thread-pool escape hatch `ARCHITECTURE.md` §5 describes is simpler here than
/// juggling the repo handle across separate async boundaries.
fn launch(repo: &Repository, cmd: &str, env: &[(&str, &Path)]) -> PushGitResult<()> {
    let mut command = shell_env::command("sh");
    command.arg("-c").arg(cmd);
    if let Some(workdir) = repo.workdir() {
        command.current_dir(workdir);
    }
    for (key, value) in env {
        command.env(key, value);
    }

    let output = command.output()?;

    // Exit status is otherwise not treated as failure — many diff tools exit non-zero on
    // "files differ" (as plain `diff` does), and there's no standardized success/failure
    // contract across merge-tool vendors either. `sh`'s own portable 126/127 exit codes
    // ("found but not executable"/"command not found") are the one exception: a real,
    // well-defined signal that nothing useful happened at all.
    if matches!(output.status.code(), Some(126) | Some(127)) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PushGitError::Subprocess {
            command: cmd.to_string(),
            message: if stderr.trim().is_empty() {
                "command not found or not executable".to_string()
            } else {
                stderr.trim().to_string()
            },
        });
    }
    Ok(())
}

/// Writes both sides of a diff to temp files (raw bytes — binary-safe, no UTF-8 assumption)
/// and launches `cmd` pointed at them via `$LOCAL`/`$REMOTE`, matching real git's own two-way
/// `difftool` environment-variable convention. Strictly read-only: nothing is written back,
/// unlike `open_merge`.
pub fn open_diff(
    repo: &Repository,
    cmd: &str,
    old: (DiffSide, &str),
    new: (DiffSide, &str),
) -> PushGitResult<()> {
    let (old_side, old_path) = old;
    let (new_side, new_path) = new;
    let old_bytes = resolve_side_bytes(repo, &old_side, old_path)?;
    let new_bytes = resolve_side_bytes(repo, &new_side, new_path)?;

    let temp_dir = tempfile::Builder::new()
        .prefix("pushgit-difftool-")
        .tempdir()?;
    let local_path = write_side(&temp_dir, "local", old_path, &old_bytes)?;
    let remote_path = write_side(&temp_dir, "remote", new_path, &new_bytes)?;

    launch(
        repo,
        cmd,
        &[
            ("LOCAL", local_path.as_path()),
            ("REMOTE", remote_path.as_path()),
        ],
    )
}

/// Writes base/ours/theirs to temp files and seeds `$MERGED` from the real, currently
/// conflicted working-tree file — libgit2's own conflicted checkout already wrote it with
/// standard `<<<<<<<`/`=======`/`>>>>>>>` markers, the same starting point real `git
/// mergetool` gives its own `$MERGED`. Launches `cmd`, then reads `$MERGED`'s final bytes back
/// — decoded lossily as UTF-8 and written+staged via `branch::write_resolved_conflict`, the
/// same call the in-app 3-way editor's "mark resolved" already uses. The real repo file is
/// never handed to the external tool directly; PushGit alone writes it, only after the tool
/// has exited.
pub fn open_merge(repo: &Repository, cmd: &str, path: &str) -> PushGitResult<()> {
    let (base, ours, theirs) = branch::conflict_raw_sides(repo, path)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| invalid("repository has no working directory"))?;
    let merged_seed = std::fs::read(workdir.join(path)).unwrap_or_default();

    let temp_dir = tempfile::Builder::new()
        .prefix("pushgit-mergetool-")
        .tempdir()?;
    let base_path = write_side(&temp_dir, "base", path, base.as_deref().unwrap_or_default())?;
    let local_path = write_side(
        &temp_dir,
        "local",
        path,
        ours.as_deref().unwrap_or_default(),
    )?;
    let remote_path = write_side(
        &temp_dir,
        "remote",
        path,
        theirs.as_deref().unwrap_or_default(),
    )?;
    let merged_path = write_side(&temp_dir, "merged", path, &merged_seed)?;

    launch(
        repo,
        cmd,
        &[
            ("BASE", base_path.as_path()),
            ("LOCAL", local_path.as_path()),
            ("REMOTE", remote_path.as_path()),
            ("MERGED", merged_path.as_path()),
        ],
    )?;

    let resolved = std::fs::read(&merged_path)?;
    let content = String::from_utf8_lossy(&resolved).into_owned();
    branch::write_resolved_conflict(repo, path, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branch::{checkout_branch, create_branch, merge_branch};
    use crate::test_support::repo_init;
    use std::fs;
    use std::path::Path as StdPath;

    fn commit_file(repo: &Repository, name: &str, content: &str) -> git2::Oid {
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

    fn set_tool_cmd(repo: &Repository, key: &str, tool: &str, section: &str, cmd: &str) {
        let mut config = repo.config().unwrap();
        config.set_str(key, tool).unwrap();
        config
            .set_str(&format!("{section}.{tool}.cmd"), cmd)
            .unwrap();
    }

    #[test]
    fn resolve_diff_command_prefers_the_app_config_override() {
        let (_dir, repo) = repo_init();
        set_tool_cmd(
            &repo,
            "diff.tool",
            "bc",
            "difftool",
            "bcompare $LOCAL $REMOTE",
        );
        let app_config = AppConfig {
            external_tools: crate::config::ExternalToolsSettings {
                diff_command: Some("meld $LOCAL $REMOTE".to_string()),
                merge_command: None,
            },
            ..AppConfig::default()
        };

        assert_eq!(
            resolve_diff_command(&repo, &app_config),
            Some("meld $LOCAL $REMOTE".to_string())
        );
    }

    #[test]
    fn resolve_diff_command_falls_back_to_gitconfig() {
        let (_dir, repo) = repo_init();
        set_tool_cmd(
            &repo,
            "diff.tool",
            "bc",
            "difftool",
            "bcompare $LOCAL $REMOTE",
        );

        assert_eq!(
            resolve_diff_command(&repo, &AppConfig::default()),
            Some("bcompare $LOCAL $REMOTE".to_string())
        );
    }

    #[test]
    fn resolve_diff_command_is_none_when_tool_name_has_no_cmd_entry() {
        let (_dir, repo) = repo_init();
        repo.config().unwrap().set_str("diff.tool", "bc").unwrap();

        assert_eq!(resolve_diff_command(&repo, &AppConfig::default()), None);
    }

    #[test]
    fn resolve_diff_command_is_none_when_unconfigured() {
        let (_dir, repo) = repo_init();
        assert_eq!(resolve_diff_command(&repo, &AppConfig::default()), None);
    }

    #[test]
    fn resolve_side_bytes_workdir_reads_the_real_file() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "on disk\n").unwrap();

        assert_eq!(
            resolve_side_bytes(&repo, &DiffSide::Workdir, "a.txt").unwrap(),
            b"on disk\n".to_vec()
        );
    }

    #[test]
    fn resolve_side_bytes_workdir_is_empty_for_a_missing_file() {
        let (_dir, repo) = repo_init();
        assert_eq!(
            resolve_side_bytes(&repo, &DiffSide::Workdir, "missing.txt").unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn resolve_side_bytes_index_reads_a_staged_blob() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "staged\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(StdPath::new("a.txt")).unwrap();
        index.write().unwrap();

        assert_eq!(
            resolve_side_bytes(&repo, &DiffSide::Index, "a.txt").unwrap(),
            b"staged\n".to_vec()
        );
    }

    #[test]
    fn resolve_side_bytes_commit_resolves_via_revparse_and_is_empty_for_an_absent_path() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "a.txt", "on head\n");

        assert_eq!(
            resolve_side_bytes(
                &repo,
                &DiffSide::Commit {
                    rev: "HEAD".to_string()
                },
                "a.txt"
            )
            .unwrap(),
            b"on head\n".to_vec()
        );
        assert_eq!(
            resolve_side_bytes(
                &repo,
                &DiffSide::Commit {
                    rev: "HEAD".to_string()
                },
                "no-such-file.txt"
            )
            .unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn open_diff_writes_both_sides_and_reports_a_missing_tool_as_an_error() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "new content\n").unwrap();

        let err = open_diff(
            &repo,
            "definitely-not-a-real-difftool-binary $LOCAL $REMOTE",
            (DiffSide::Empty, "a.txt"),
            (DiffSide::Workdir, "a.txt"),
        )
        .unwrap_err();

        assert!(matches!(err, PushGitError::Subprocess { .. }));
    }

    #[test]
    fn open_diff_writes_the_expected_content_into_temp_files() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "a.txt", "old content\n");
        fs::write(dir.path().join("a.txt"), "new content\n").unwrap();

        // Captures both temp paths via the shell command itself, since they're only known
        // inside `launch`.
        let capture = dir.path().join("captured.txt");
        let cmd = format!(
            "printf '%s\\n%s\\n' \"$(cat \"$LOCAL\")\" \"$(cat \"$REMOTE\")\" > {}",
            capture.display()
        );

        open_diff(
            &repo,
            &cmd,
            (
                DiffSide::Commit {
                    rev: "HEAD".to_string(),
                },
                "a.txt",
            ),
            (DiffSide::Workdir, "a.txt"),
        )
        .unwrap();

        let captured = fs::read_to_string(&capture).unwrap();
        assert_eq!(captured, "old content\nnew content\n");
    }

    fn conflicted_repo() -> (tempfile::TempDir, Repository) {
        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        commit_file(&repo, "shared.txt", "main version\n");
        checkout_branch(&repo, "feature").unwrap();
        commit_file(&repo, "shared.txt", "feature version\n");
        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();
        (dir, repo)
    }

    #[test]
    fn open_merge_writes_the_tools_output_back_and_stages_it() {
        let (_dir, repo) = conflicted_repo();
        // Simulates the external merge tool overwriting `$MERGED` with its own resolution.
        let cmd = "echo 'resolved by tool' > \"$MERGED\"";

        open_merge(&repo, cmd, "shared.txt").unwrap();

        assert_eq!(
            fs::read_to_string(repo.workdir().unwrap().join("shared.txt")).unwrap(),
            "resolved by tool\n"
        );
        assert!(branch::list_conflicts(&repo).unwrap().is_empty());
    }

    #[test]
    fn open_merge_seeds_merged_from_the_real_conflict_marker_file() {
        let (dir, repo) = conflicted_repo();
        let on_disk = fs::read_to_string(dir.path().join("shared.txt")).unwrap();
        assert!(on_disk.contains("<<<<<<<"));

        let capture = dir.path().join("captured.txt");
        let cmd = format!("cp \"$MERGED\" {}", capture.display());

        open_merge(&repo, &cmd, "shared.txt").unwrap();

        assert_eq!(fs::read_to_string(&capture).unwrap(), on_disk);
    }
}
