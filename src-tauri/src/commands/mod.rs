// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Thin `#[tauri::command]` wrappers over the modules in this crate; owns `spawn_blocking`
//! dispatch for git2 calls and the `Channel`/progress-streaming plumbing.

use std::path::Path;

use git2::Repository;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ai;
use crate::blame::{self, BlameLine, FileHistoryEntry};
use crate::branch::{
    self, BranchInfo, CherryPickOutcome, MergeOutcome, RebaseOutcome, RepoState, ResetMode,
};
use crate::cherry_pick_range;
use crate::config;
use crate::diff::{self, ConflictSides, FileDiff, Hunk};
use crate::error::{PushGitError, PushGitResult};
use crate::graph::{CommitGraphPage, GraphFilter};
use crate::hooks::HookOutputLine;
use crate::interactive_rebase::{self, RebaseCommitSummary, RebaseStep};
use crate::maintenance::{self, RepoHealth};
use crate::remote::{self, GitVersionCheck, RemoteProgress};
use crate::repo;
use crate::stage;
use crate::stash::{self, StashEntry};
use crate::state::AppState;
use crate::undo::{OperationSummary, UndoRedoStatus};
use crate::update_check;
use crate::watcher;
use crate::workflow::{self, FinishOutcome, WorkflowBranchKind, WorkflowConfig};

/// Runs `op` against a freshly opened `repo_path`, recording an undo/redo entry labeled
/// `label` beforehand — the shared wrapper behind every "destructive/hard-to-reverse"
/// command. Trivially-reversible actions (stage/unstage) don't go
/// through this; a command whose outcome can be "didn't actually happen" (merge/rebase
/// ending in conflicts) calls `state.undo_log.capture`/`push` directly instead, so it can
/// skip the `push` when nothing should be undoable. Synchronous, like `UndoLog` itself
/// (`undo/stack.rs`'s doc comment) — no `.await` anywhere in here to hold a non-`Send`
/// `&Repository` across.
fn with_undo<T>(
    state: &State<'_, AppState>,
    repo_path: &str,
    label: impl Into<String>,
    op: impl FnOnce(&Repository) -> PushGitResult<T>,
) -> PushGitResult<T> {
    let repo = repo::open(Path::new(repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let result = op(&repo)?;
    state.undo_log.push(&repo, repo_path, label, snapshot)?;
    Ok(result)
}

/// Like `with_undo`, for the handful of operations (stash) whose git2 API needs `&mut
/// Repository`.
fn with_undo_mut<T>(
    state: &State<'_, AppState>,
    repo_path: &str,
    label: impl Into<String>,
    op: impl FnOnce(&mut Repository) -> PushGitResult<T>,
) -> PushGitResult<T> {
    let mut repo = repo::open(Path::new(repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let result = op(&mut repo)?;
    state.undo_log.push(&repo, repo_path, label, snapshot)?;
    Ok(result)
}

/// Smoke-test command proving the frontend → IPC → repo/ round trip; returns the
/// repository's working directory path.
#[tauri::command]
pub fn open_repository(path: String) -> PushGitResult<String> {
    let repo = repo::open(Path::new(&path))?;
    let workdir = repo.workdir().unwrap_or_else(|| repo.path());
    Ok(workdir.display().to_string())
}

/// Opens a commit graph session for `repo_path` and returns its session id.
#[tauri::command]
pub async fn graph_open(
    repo_path: String,
    filter: GraphFilter,
    state: State<'_, AppState>,
) -> PushGitResult<String> {
    state
        .graph_sessions
        .open(Path::new(&repo_path), &filter)
        .await
}

/// Returns the next page of rows for an open graph session, resuming the lane-assignment
/// sweep from where the previous call left off.
#[tauri::command]
pub async fn graph_page(
    session_id: String,
    page_size: u32,
    state: State<'_, AppState>,
) -> PushGitResult<CommitGraphPage> {
    state.graph_sessions.page(&session_id, page_size).await
}

/// Tears down a graph session, called from the frontend's unmount/teardown.
#[tauri::command]
pub async fn graph_close(session_id: String, state: State<'_, AppState>) -> PushGitResult<()> {
    state.graph_sessions.close(&session_id).await;
    Ok(())
}

/// Unstaged working-directory changes (workdir vs. index).
#[tauri::command]
pub fn diff_unstaged(repo_path: String) -> PushGitResult<Vec<FileDiff>> {
    diff::diff_unstaged(&repo::open(Path::new(&repo_path))?)
}

/// Staged changes (index vs. HEAD).
#[tauri::command]
pub fn diff_staged(repo_path: String) -> PushGitResult<Vec<FileDiff>> {
    diff::diff_staged(&repo::open(Path::new(&repo_path))?)
}

/// A single commit's changes against its first parent.
#[tauri::command]
pub fn diff_commit(repo_path: String, commit_oid: String) -> PushGitResult<Vec<FileDiff>> {
    diff::diff_commit(&repo::open(Path::new(&repo_path))?, &commit_oid)
}

/// The diff between two arbitrary commits (e.g. ctrl-click two graph rows).
#[tauri::command]
pub fn diff_between_commits(
    repo_path: String,
    from_oid: String,
    to_oid: String,
) -> PushGitResult<Vec<FileDiff>> {
    diff::diff_between_commits(&repo::open(Path::new(&repo_path))?, &from_oid, &to_oid)
}

/// A commit's changes against the live working directory — the graph's commit-dot context
/// menu's "Diff against working tree".
#[tauri::command]
pub fn diff_commit_to_workdir(
    repo_path: String,
    commit_oid: String,
) -> PushGitResult<Vec<FileDiff>> {
    diff::diff_commit_to_workdir(&repo::open(Path::new(&repo_path))?, &commit_oid)
}

/// Per-line authorship for `path` as of HEAD.
#[tauri::command]
pub fn blame_file(repo_path: String, path: String) -> PushGitResult<Vec<BlameLine>> {
    blame::blame_file(&repo::open(Path::new(&repo_path))?, &path)
}

/// The commits that changed `path`, newest first.
#[tauri::command]
pub fn file_history(repo_path: String, path: String) -> PushGitResult<Vec<FileHistoryEntry>> {
    blame::file_history(&repo::open(Path::new(&repo_path))?, &path)
}

/// Stages a whole file (or its deletion, if it no longer exists on disk).
#[tauri::command]
pub fn stage_file(repo_path: String, path: String) -> PushGitResult<()> {
    stage::stage_file(&repo::open(Path::new(&repo_path))?, &path)
}

/// Unstages a whole file, resetting its index entry back to HEAD.
#[tauri::command]
pub fn unstage_file(repo_path: String, path: String) -> PushGitResult<()> {
    stage::unstage_file(&repo::open(Path::new(&repo_path))?, &path)
}

/// Stages a single hunk from the unstaged diff without touching the rest of the file.
#[tauri::command]
pub fn stage_hunk(repo_path: String, path: String, hunk: Hunk) -> PushGitResult<()> {
    stage::stage_hunk(&repo::open(Path::new(&repo_path))?, &path, &hunk)
}

/// Unstages a single hunk from the staged diff without touching the rest of the file.
#[tauri::command]
pub fn unstage_hunk(repo_path: String, path: String, hunk: Hunk) -> PushGitResult<()> {
    stage::unstage_hunk(&repo::open(Path::new(&repo_path))?, &path, &hunk)
}

/// Stages only `line_indices` (positions into `hunk.lines`) from the unstaged diff, leaving
/// the rest of the hunk unstaged.
#[tauri::command]
pub fn stage_lines(
    repo_path: String,
    path: String,
    hunk: Hunk,
    line_indices: Vec<usize>,
) -> PushGitResult<()> {
    stage::stage_lines(
        &repo::open(Path::new(&repo_path))?,
        &path,
        &hunk,
        &line_indices,
    )
}

/// Unstages only `line_indices` (positions into `hunk.lines`) from the staged diff, leaving
/// the rest of the file's staged changes untouched.
#[tauri::command]
pub fn unstage_lines(
    repo_path: String,
    path: String,
    hunk: Hunk,
    line_indices: Vec<usize>,
) -> PushGitResult<()> {
    stage::unstage_lines(
        &repo::open(Path::new(&repo_path))?,
        &path,
        &hunk,
        &line_indices,
    )
}

/// Commits the current index tree; `amend` rewrites HEAD in place instead of creating a
/// new commit. `skip_hooks` bypasses `pre-commit`/`commit-msg` (real `git commit
/// --no-verify`'s equivalent) — see `hooks/mod.rs` and `stage::commit`. `async` + `spawn_blocking`
/// because a `pre-commit`/`commit-msg` hook can run arbitrary, arbitrarily slow user scripts —
/// running that synchronously (as a plain non-`async` command) would block the WebView's IPC
/// dispatch thread, which on Linux/WebKitGTK is the GTK main loop, freezing the whole window.
/// `hook_output` streams each hook's output lines live as they're produced (see
/// `hooks::output::stream_command`), so the frontend can show a running transcript instead of
/// only the final rejection message on failure.
#[tauri::command]
pub async fn commit(
    repo_path: String,
    message: String,
    amend: bool,
    skip_hooks: bool,
    app: AppHandle,
    hook_output: Channel<HookOutputLine>,
) -> PushGitResult<String> {
    let label = if amend {
        "Amend commit".to_string()
    } else {
        format!("Commit '{message}'")
    };
    let oid = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        with_undo(&state, &repo_path, label, |repo| {
            stage::commit(repo, &message, amend, skip_hooks, &mut |line| {
                let _ = hook_output.send(line);
            })
        })
    })
    .await
    .map_err(|e| PushGitError::Invalid(format!("commit task panicked: {e}")))??;
    Ok(oid.to_string())
}

/// The current HEAD commit's message, so the frontend can pre-fill the amend box.
#[tauri::command]
pub fn head_commit_message(repo_path: String) -> PushGitResult<Option<String>> {
    stage::head_commit_message(&repo::open(Path::new(&repo_path))?)
}

/// The `commit.template` file's content, if configured, so the frontend can pre-fill a
/// fresh (non-amend) commit box.
#[tauri::command]
pub fn commit_message_template(repo_path: String) -> PushGitResult<Option<String>> {
    stage::commit_message_template(&repo::open(Path::new(&repo_path))?)
}

/// Local branches with ahead/behind counts against their upstream, if any.
#[tauri::command]
pub fn list_branches(repo_path: String) -> PushGitResult<Vec<BranchInfo>> {
    branch::list_branches(&repo::open(Path::new(&repo_path))?)
}

/// Creates a local branch at `at` (any commit-ish), or at HEAD if `at` is not given.
#[tauri::command]
pub fn create_branch(repo_path: String, name: String, at: Option<String>) -> PushGitResult<()> {
    branch::create_branch(&repo::open(Path::new(&repo_path))?, &name, at.as_deref())
}

#[tauri::command]
pub fn checkout_branch(
    repo_path: String,
    name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(&state, &repo_path, format!("Checkout '{name}'"), |repo| {
        branch::checkout_branch(repo, &name)
    })
}

/// Checks out an arbitrary commit directly (detached HEAD) — the graph's commit-dot context
/// menu's "Checkout this commit".
#[tauri::command]
pub fn checkout_commit(
    repo_path: String,
    oid: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(&state, &repo_path, format!("Checkout {oid}"), |repo| {
        branch::checkout_commit(repo, &oid)
    })
}

/// Checks out a remote-tracking branch (e.g. `"origin/feature"`), creating a local tracking
/// branch first if none exists — the graph's remote-branch-badge context menu's "Checkout".
#[tauri::command]
pub fn checkout_remote_branch(
    repo_path: String,
    remote_branch_name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Checkout '{remote_branch_name}'"),
        |repo| branch::checkout_remote_branch(repo, &remote_branch_name),
    )
}

#[tauri::command]
pub fn rename_branch(
    repo_path: String,
    old_name: String,
    new_name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Rename '{old_name}' to '{new_name}'"),
        |repo| branch::rename_branch(repo, &old_name, &new_name),
    )
}

#[tauri::command]
pub fn delete_branch(
    repo_path: String,
    name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Delete branch '{name}'"),
        |repo| branch::delete_branch(repo, &name),
    )
}

/// Merges `branch_name` into HEAD (fast-forward when possible, otherwise a real 3-way
/// merge). Conflicts are left staged for `list_conflicts`/`resolve_conflict`, not aborted —
/// and, since the merge hasn't actually finished, don't get an undo/redo entry either; only
/// a clean outcome does.
#[tauri::command]
pub fn merge_branch(
    repo_path: String,
    branch_name: String,
    state: State<'_, AppState>,
) -> PushGitResult<MergeOutcome> {
    let repo = repo::open(Path::new(&repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let outcome = branch::merge_branch(&repo, &branch_name)?;
    if !matches!(outcome, MergeOutcome::Conflicts(_)) {
        state.undo_log.push(
            &repo,
            &repo_path,
            format!("Merge '{branch_name}'"),
            snapshot,
        )?;
    }
    Ok(outcome)
}

#[tauri::command]
pub fn abort_merge(repo_path: String) -> PushGitResult<()> {
    branch::abort_merge(&repo::open(Path::new(&repo_path))?)
}

/// Applies `commit_oid` onto the current HEAD as a new commit. Same conflict-skips-the-
/// undo-entry rule as `merge_branch` — a conflicted cherry-pick is finished later via the
/// ordinary `commit` command, which detects `CHERRY_PICK_HEAD` itself.
#[tauri::command]
pub fn cherry_pick_commit(
    repo_path: String,
    commit_oid: String,
    state: State<'_, AppState>,
) -> PushGitResult<CherryPickOutcome> {
    let repo = repo::open(Path::new(&repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let outcome = branch::cherry_pick(&repo, &commit_oid)?;
    if !matches!(outcome, CherryPickOutcome::Conflicts { .. }) {
        state.undo_log.push(
            &repo,
            &repo_path,
            format!("Cherry-pick {commit_oid}"),
            snapshot,
        )?;
    }
    Ok(outcome)
}

#[tauri::command]
pub fn abort_cherry_pick(repo_path: String) -> PushGitResult<()> {
    branch::abort_cherry_pick(&repo::open(Path::new(&repo_path))?)
}

/// Whether a paused cherry-pick was started by `cherry_pick_range` (a multi-commit
/// sequence) rather than the single-commit `cherry_pick_commit` — see
/// `branch::is_multi_cherry_pick_in_progress` for why `BranchSidebar` needs this.
#[tauri::command]
pub fn is_multi_cherry_pick_in_progress(repo_path: String) -> PushGitResult<bool> {
    Ok(branch::is_multi_cherry_pick_in_progress(&repo::open(
        Path::new(&repo_path),
    )?))
}

/// Resets the current branch to `target` (`git reset --soft/--mixed/--hard`) — the commit
/// graph's drag-to-reset gesture.
#[tauri::command]
pub fn reset_to(
    repo_path: String,
    target: String,
    mode: ResetMode,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(&state, &repo_path, format!("Reset to {target}"), |repo| {
        branch::reset_to(repo, &target, mode)
    })
}

/// Applies `commit_oids`, in the given order, onto HEAD — stops at the first conflict. The
/// pre-op snapshot is only recorded as a real undo entry once the whole range resolves
/// (`UndoLog::hold_pending`), the same multi-step pattern `start_interactive_rebase` uses.
#[tauri::command]
pub async fn cherry_pick_range(
    repo_path: String,
    commit_oids: Vec<String>,
    state: State<'_, AppState>,
) -> PushGitResult<CherryPickOutcome> {
    {
        let repo = repo::open(Path::new(&repo_path))?;
        let snapshot = state.undo_log.capture(&repo)?;
        state.undo_log.hold_pending(
            &repo,
            &repo_path,
            format!("Cherry-pick {} commits", commit_oids.len()),
            snapshot,
        )?;
    }

    let outcome = cherry_pick_range::cherry_pick_range(Path::new(&repo_path), &commit_oids).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(
        &repo,
        &repo_path,
        matches!(outcome, CherryPickOutcome::CherryPicked { .. }),
    )?;
    Ok(outcome)
}

/// Resumes a cherry-pick range paused by a conflict, once the caller has resolved and
/// staged it.
#[tauri::command]
pub async fn continue_cherry_pick_range(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<CherryPickOutcome> {
    let outcome = cherry_pick_range::continue_cherry_pick_range(Path::new(&repo_path)).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(
        &repo,
        &repo_path,
        matches!(outcome, CherryPickOutcome::CherryPicked { .. }),
    )?;
    Ok(outcome)
}

/// Aborts an in-progress cherry-pick range, restoring HEAD to where it was before
/// `cherry_pick_range` ran.
#[tauri::command]
pub async fn abort_cherry_pick_range(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    cherry_pick_range::abort_cherry_pick_range(Path::new(&repo_path)).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(&repo, &repo_path, false)?;
    Ok(())
}

/// Non-interactive rebase of the current branch onto `onto`, stopping at the first
/// conflict. Same conflict-skips-the-undo-entry rule as `merge_branch`.
#[tauri::command]
pub fn rebase_branch(
    repo_path: String,
    onto: String,
    state: State<'_, AppState>,
) -> PushGitResult<RebaseOutcome> {
    let repo = repo::open(Path::new(&repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let outcome = branch::rebase_branch(&repo, &onto)?;
    if !matches!(outcome, RebaseOutcome::Conflicts(_)) {
        state
            .undo_log
            .push(&repo, &repo_path, format!("Rebase onto '{onto}'"), snapshot)?;
    }
    Ok(outcome)
}

#[tauri::command]
pub fn abort_rebase(repo_path: String) -> PushGitResult<()> {
    branch::abort_rebase(&repo::open(Path::new(&repo_path))?)
}

/// Resumes a rebase paused by a conflict, once the caller has resolved and staged it. Same
/// conflict-skips-the-undo-entry rule as `merge_branch`/`rebase_branch`.
#[tauri::command]
pub fn continue_rebase(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<RebaseOutcome> {
    let repo = repo::open(Path::new(&repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let outcome = branch::continue_rebase(&repo)?;
    if !matches!(outcome, RebaseOutcome::Conflicts(_)) {
        state.undo_log.push(&repo, &repo_path, "Rebase", snapshot)?;
    }
    Ok(outcome)
}

/// The commits between `onto` and HEAD, oldest first, for the interactive rebase editor's
/// reorderable list — see `interactive_rebase` for why this is a wholly separate feature
/// from the plain `rebase_branch`/`continue_rebase`/`abort_rebase` above.
#[tauri::command]
pub fn list_rebase_commits(
    repo_path: String,
    onto: String,
) -> PushGitResult<Vec<RebaseCommitSummary>> {
    interactive_rebase::list_rebase_commits(&repo::open(Path::new(&repo_path))?, &onto)
}

/// Whether a paused rebase (`repository_state() === "rebase"`) was started by *this*
/// module rather than plain `rebase_branch` — lets `BranchSidebar` suppress its own
/// plain-rebase conflict banner (whose Continue/Abort would call the wrong backend) in
/// favor of the interactive rebase editor's own recovery controls.
#[tauri::command]
pub fn is_interactive_rebase_in_progress(repo_path: String) -> PushGitResult<bool> {
    interactive_rebase::is_interactive_rebase_in_progress(Path::new(&repo_path))
}

/// Starts (and, if nothing conflicts, finishes) an interactive rebase of the current branch
/// onto `onto`, replaying `steps` in order. The pre-rebase snapshot is only recorded as a
/// real undo entry once the whole sequence resolves — see `UndoLog::hold_pending`.
#[tauri::command]
pub async fn start_interactive_rebase(
    repo_path: String,
    onto: String,
    steps: Vec<RebaseStep>,
    state: State<'_, AppState>,
) -> PushGitResult<RebaseOutcome> {
    {
        let repo = repo::open(Path::new(&repo_path))?;
        let snapshot = state.undo_log.capture(&repo)?;
        state.undo_log.hold_pending(
            &repo,
            &repo_path,
            format!("Interactive rebase onto '{onto}'"),
            snapshot,
        )?;
    }

    let outcome =
        interactive_rebase::start_interactive_rebase(Path::new(&repo_path), &onto, &steps).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(
        &repo,
        &repo_path,
        matches!(outcome, RebaseOutcome::Completed),
    )?;
    Ok(outcome)
}

/// Resumes an interactive rebase paused by a conflict, once the caller has resolved and
/// staged it.
#[tauri::command]
pub async fn continue_interactive_rebase(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<RebaseOutcome> {
    let outcome = interactive_rebase::continue_interactive_rebase(Path::new(&repo_path)).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(
        &repo,
        &repo_path,
        matches!(outcome, RebaseOutcome::Completed),
    )?;
    Ok(outcome)
}

/// Aborts an in-progress interactive rebase, restoring the branch to where it was before
/// `start_interactive_rebase` ran.
#[tauri::command]
pub async fn abort_interactive_rebase(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    interactive_rebase::abort_interactive_rebase(Path::new(&repo_path)).await?;

    let repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.resolve_pending(&repo, &repo_path, false)?;
    Ok(())
}

#[tauri::command]
pub fn list_conflicts(repo_path: String) -> PushGitResult<Vec<String>> {
    branch::list_conflicts(&repo::open(Path::new(&repo_path))?)
}

/// Whether a merge or rebase is currently paused, so the frontend can offer the matching
/// recovery action (abort merge / abort or continue rebase) instead of guessing.
#[tauri::command]
pub fn repository_state(repo_path: String) -> PushGitResult<RepoState> {
    Ok(branch::repo_state(&repo::open(Path::new(&repo_path))?))
}

/// Marks a conflicted path resolved by staging its current working-tree content.
#[tauri::command]
pub fn resolve_conflict(
    repo_path: String,
    path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Resolve conflict in '{path}'"),
        |repo| branch::resolve_conflict(repo, &path),
    )
}

/// The base/ours/theirs content of one conflicted path, for the 3-way conflict editor.
#[tauri::command]
pub fn conflict_sides(repo_path: String, path: String) -> PushGitResult<ConflictSides> {
    branch::conflict_sides(&repo::open(Path::new(&repo_path))?, &path)
}

/// Writes the conflict editor's resolved content to the working tree and stages it.
#[tauri::command]
pub fn write_resolved_conflict(
    repo_path: String,
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Resolve conflict in '{path}'"),
        |repo| branch::write_resolved_conflict(repo, &path, &content),
    )
}

/// Resolves a delete/modify conflict by deleting the path.
#[tauri::command]
pub fn resolve_conflict_as_deleted(
    repo_path: String,
    path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Resolve conflict in '{path}' (deleted)"),
        |repo| branch::resolve_conflict_as_deleted(repo, &path),
    )
}

/// Discards a file's uncommitted changes (staged and unstaged) back to HEAD, or removes it
/// entirely if HEAD has no record of it — otherwise unrecoverable, so this always goes
/// through the undo/redo stack.
#[tauri::command]
pub fn discard_file_changes(
    repo_path: String,
    path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Discard changes to '{path}'"),
        |repo| stage::discard_file_changes(repo, &path),
    )
}

/// Undoes the most recently recorded destructive operation, restoring the repo to exactly
/// how it looked beforehand. Errors if there's nothing to undo.
#[tauri::command]
pub fn undo_last_operation(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<OperationSummary> {
    let mut repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.undo(&mut repo, &repo_path)
}

/// Re-applies the most recently undone operation. Errors if there's nothing to redo.
#[tauri::command]
pub fn redo_last_operation(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<OperationSummary> {
    let mut repo = repo::open(Path::new(&repo_path))?;
    state.undo_log.redo(&mut repo, &repo_path)
}

/// Whether undo/redo are available for `repo_path`, and what each would do — drives the
/// frontend's Undo/Redo button labels and disabled state.
#[tauri::command]
pub fn undo_redo_status(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<UndoRedoStatus> {
    Ok(state.undo_log.status(&repo_path))
}

#[tauri::command]
pub fn list_tags(repo_path: String) -> PushGitResult<Vec<String>> {
    branch::list_tags(&repo::open(Path::new(&repo_path))?)
}

/// Creates a tag at `at` (or HEAD if not given); annotated if `message` is provided,
/// otherwise lightweight.
#[tauri::command]
pub fn create_tag(
    repo_path: String,
    name: String,
    at: Option<String>,
    message: Option<String>,
) -> PushGitResult<()> {
    branch::create_tag(
        &repo::open(Path::new(&repo_path))?,
        &name,
        at.as_deref(),
        message.as_deref(),
    )
}

#[tauri::command]
pub fn delete_tag(
    repo_path: String,
    name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(&state, &repo_path, format!("Delete tag '{name}'"), |repo| {
        branch::delete_tag(repo, &name)
    })
}

/// Force-moves an existing tag to `to` (any commit-ish) — the graph's drag-a-tag-onto-a-
/// commit-or-branch-badge gesture.
#[tauri::command]
pub fn move_tag(repo_path: String, name: String, to: String) -> PushGitResult<()> {
    branch::move_tag(&repo::open(Path::new(&repo_path))?, &name, &to)
}

#[tauri::command]
pub fn rename_tag(
    repo_path: String,
    old_name: String,
    new_name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(
        &state,
        &repo_path,
        format!("Rename tag '{old_name}' to '{new_name}'"),
        |repo| branch::rename_tag(repo, &old_name, &new_name),
    )
}

/// The repo's GitFlow config (`.git/config`'s `[gitflow ...]` keys), or `None` if it hasn't
/// been set up yet — drives `SettingsPanel`'s "Set up GitFlow" form and `WorkflowPanel`'s
/// initialized/uninitialized state.
#[tauri::command]
pub fn detect_workflow(repo_path: String) -> PushGitResult<Option<WorkflowConfig>> {
    workflow::detect_workflow(&repo::open(Path::new(&repo_path))?)
}

/// Writes `config` as this repo's GitFlow setup (equivalent to `git flow init`), creating
/// `develop` from `main`'s tip if it doesn't already exist. Not undo-tracked, same as
/// `set_commit_template_path`/`create_branch` individually aren't — a config write and an
/// initial branch creation are both trivially redoable by hand, not "hard to reverse".
#[tauri::command]
pub fn init_workflow(repo_path: String, config: WorkflowConfig) -> PushGitResult<()> {
    workflow::init_workflow(&repo::open(Path::new(&repo_path))?, &config)
}

/// Starts a GitFlow feature/release/hotfix branch — undo-tracked because, unlike plain
/// `create_branch`, this also checks the new branch out (moving HEAD).
#[tauri::command]
pub fn start_workflow_branch(
    repo_path: String,
    config: WorkflowConfig,
    kind: WorkflowBranchKind,
    name: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo(&state, &repo_path, format!("Start '{name}'"), |repo| {
        workflow::start_branch(repo, &config, kind, &name)
    })
}

/// Finishes a GitFlow feature/release/hotfix branch. Same conflict-skips-the-undo-entry rule
/// as `merge_branch`: a paused finish (`FinishOutcome::Conflicts`) hasn't actually finished,
/// so it doesn't get an undo entry — the caller resolves the conflict through the normal
/// merge-conflict flow and calls this again with the same arguments to pick the remaining
/// steps back up.
#[tauri::command]
pub fn finish_workflow_branch(
    repo_path: String,
    config: WorkflowConfig,
    kind: WorkflowBranchKind,
    name: String,
    state: State<'_, AppState>,
) -> PushGitResult<FinishOutcome> {
    let repo = repo::open(Path::new(&repo_path))?;
    let snapshot = state.undo_log.capture(&repo)?;
    let outcome = workflow::finish_branch(&repo, &config, kind, &name)?;
    if !matches!(outcome, FinishOutcome::Conflicts(_)) {
        state
            .undo_log
            .push(&repo, &repo_path, format!("Finish '{name}'"), snapshot)?;
    }
    Ok(outcome)
}

/// Checks the system `git` binary's version against the CVE-2024-32002 patch list —
/// call this before the first clone/fetch/pull/push of a session.
#[tauri::command]
pub async fn check_git_version() -> PushGitResult<GitVersionCheck> {
    remote::check_git_version().await
}

/// A rough repo-health snapshot (loose object/pack counts and sizes) from `git
/// count-objects -v` — the same numbers `git gc`'s own "should I repack" heuristic uses.
#[tauri::command]
pub async fn repo_health(repo_path: String) -> PushGitResult<RepoHealth> {
    maintenance::repo_health(Path::new(&repo_path)).await
}

/// Runs plain `git gc` on the repository.
#[tauri::command]
pub async fn run_gc(repo_path: String) -> PushGitResult<()> {
    maintenance::run_gc(Path::new(&repo_path)).await
}

/// Fetches `remote_name`, streaming progress through `progress` and registering a fresh
/// cancellation token for `repo_path` before starting — `cancel_remote_operation` sets it.
#[tauri::command]
pub async fn fetch(
    repo_path: String,
    remote_name: String,
    progress: Channel<RemoteProgress>,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    let cancel = state
        .remote_cancellation
        .register(Path::new(&repo_path))
        .await;
    remote::fetch(Path::new(&repo_path), &remote_name, &progress, &cancel).await
}

/// Fetches, then merges the current branch's configured upstream into HEAD. Same
/// conflict-skips-the-undo-entry rule as `merge_branch`.
#[tauri::command]
pub async fn pull(
    repo_path: String,
    remote_name: String,
    progress: Channel<RemoteProgress>,
    state: State<'_, AppState>,
) -> PushGitResult<MergeOutcome> {
    let cancel = state
        .remote_cancellation
        .register(Path::new(&repo_path))
        .await;
    let snapshot = {
        let repo = repo::open(Path::new(&repo_path))?;
        state.undo_log.capture(&repo)?
    };

    let outcome = remote::pull(Path::new(&repo_path), &remote_name, &progress, &cancel).await?;

    if !matches!(outcome, MergeOutcome::Conflicts(_)) {
        let repo = repo::open(Path::new(&repo_path))?;
        state.undo_log.push(
            &repo,
            &repo_path,
            format!("Pull from '{remote_name}'"),
            snapshot,
        )?;
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn push(
    repo_path: String,
    remote_name: String,
    branch_name: String,
    force: bool,
    progress: Channel<RemoteProgress>,
    hook_output: Channel<HookOutputLine>,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    let cancel = state
        .remote_cancellation
        .register(Path::new(&repo_path))
        .await;
    remote::push(
        Path::new(&repo_path),
        &remote_name,
        &branch_name,
        force,
        &progress,
        &hook_output,
        &cancel,
    )
    .await
}

/// Cancels whatever fetch/pull/push is currently in flight for `repo_path`, if any — a
/// no-op if nothing is (already finished, or never started).
#[tauri::command]
pub async fn cancel_remote_operation(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    state
        .remote_cancellation
        .cancel(Path::new(&repo_path))
        .await;
    Ok(())
}

#[tauri::command]
pub async fn clone_repository(url: String, dest: String) -> PushGitResult<()> {
    remote::clone(&url, Path::new(&dest)).await
}

#[tauri::command]
pub fn create_stash(repo_path: String, message: Option<String>) -> PushGitResult<String> {
    stash::create_stash(&mut repo::open(Path::new(&repo_path))?, message.as_deref())
}

/// Stashes only `paths`' changes, leaving every other file's uncommitted changes as-is.
#[tauri::command]
pub fn create_stash_for_paths(
    repo_path: String,
    message: Option<String>,
    paths: Vec<String>,
) -> PushGitResult<String> {
    stash::create_stash_for_paths(
        &mut repo::open(Path::new(&repo_path))?,
        message.as_deref(),
        &paths,
    )
}

/// Renames a stash by rewriting its reflog message — a stash's "name" is its message,
/// there's no separate rename concept in git itself.
#[tauri::command]
pub fn rename_stash(repo_path: String, index: usize, new_message: String) -> PushGitResult<()> {
    stash::rename_stash(&mut repo::open(Path::new(&repo_path))?, index, &new_message)
}

#[tauri::command]
pub fn list_stashes(repo_path: String) -> PushGitResult<Vec<StashEntry>> {
    stash::list_stashes(&mut repo::open(Path::new(&repo_path))?)
}

/// Applies a stash without removing it from the stash list.
#[tauri::command]
pub fn apply_stash(
    repo_path: String,
    index: usize,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo_mut(
        &state,
        &repo_path,
        format!("Apply stash #{index}"),
        |repo| stash::apply_stash(repo, index),
    )
}

/// Applies a stash and removes it from the stash list.
#[tauri::command]
pub fn pop_stash(repo_path: String, index: usize, state: State<'_, AppState>) -> PushGitResult<()> {
    with_undo_mut(&state, &repo_path, format!("Pop stash #{index}"), |repo| {
        stash::pop_stash(repo, index)
    })
}

/// Removes a stash without applying it — unrecoverable via plain git, but protected by the
/// undo/redo stack here.
#[tauri::command]
pub fn drop_stash(
    repo_path: String,
    index: usize,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    with_undo_mut(&state, &repo_path, format!("Drop stash #{index}"), |repo| {
        stash::drop_stash(repo, index)
    })
}

/// Starts watching `repo_path` for relevant changes,
/// emitting a `repo-changed` event the frontend can listen for to trigger a refresh.
/// Replaces any previously running watcher — PushGit watches one repo at a time.
#[tauri::command]
pub async fn start_repo_watcher(
    repo_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    let debouncer = watcher::start_watching(Path::new(&repo_path), move || {
        let _ = app.emit("repo-changed", ());
    })?;
    *state.repo_watcher.lock().await = Some(debouncer);
    Ok(())
}

/// Stops watching, e.g. when the frontend closes the current repo.
#[tauri::command]
pub async fn stop_repo_watcher(state: State<'_, AppState>) -> PushGitResult<()> {
    *state.repo_watcher.lock().await = None;
    Ok(())
}

/// App-wide settings, e.g. the max-commits-
/// rendered default. Never fails — a missing/corrupt config file degrades to defaults, per
/// `config`'s own module doc.
#[tauri::command]
pub fn get_app_config() -> config::AppConfig {
    config::load_app_config()
}

/// Clamps and persists a new app-wide max-commits-rendered default, returning the resulting
/// config so the frontend doesn't need a second round-trip to see the clamped value. Loads the
/// existing config first so other fields (e.g. `reduce_motion`) aren't clobbered back to their
/// defaults.
#[tauri::command]
pub fn set_max_commits_rendered(value: u32) -> config::AppConfig {
    let config = config::AppConfig {
        max_commits_rendered: config::clamp_max_commits_rendered(value),
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Persists the app-wide "reduce motion" preference, returning the resulting config. Loads the
/// existing config first, same reasoning as `set_max_commits_rendered`.
#[tauri::command]
pub fn set_reduce_motion(value: bool) -> config::AppConfig {
    let config = config::AppConfig {
        reduce_motion: value,
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Persists whether the frontend's periodic auto-fetch timer is enabled, returning the
/// resulting config. Loads the existing config first, same reasoning as
/// `set_max_commits_rendered`.
#[tauri::command]
pub fn set_auto_fetch_enabled(value: bool) -> config::AppConfig {
    let config = config::AppConfig {
        auto_fetch_enabled: value,
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Clamps and persists the auto-fetch interval (minutes), returning the resulting config so
/// the frontend doesn't need a second round-trip to see the clamped value.
#[tauri::command]
pub fn set_auto_fetch_interval_minutes(value: u32) -> config::AppConfig {
    let config = config::AppConfig {
        auto_fetch_interval_minutes: config::clamp_auto_fetch_interval_minutes(value),
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Persists whether `HookOutputModal` opens immediately when a commit/push hook starts
/// running, versus staying hidden until the operation fails. Same load-existing-config-first
/// reasoning as `set_reduce_motion`.
#[tauri::command]
pub fn set_show_hook_output_always(value: bool) -> config::AppConfig {
    let config = config::AppConfig {
        show_hook_output_always: value,
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Checks GitHub's public releases API for a newer PushGit release, returning `Some` only if
/// one exists — collapses "no update" and "the check itself failed" (offline, GitHub down) to
/// the same `None`, since the frontend has nothing more useful to do with the distinction.
/// Always runs when called; the frontend is responsible for not calling this at all when
/// `AppConfig::check_for_updates_enabled` is off, same split as auto-fetch's enabled flag.
#[tauri::command]
pub async fn check_for_update() -> Option<update_check::ReleaseInfo> {
    update_check::check_for_update(env!("CARGO_PKG_VERSION")).await
}

/// Persists whether the launch-time update check runs at all, returning the resulting config.
/// Same load-existing-config-first reasoning as `set_auto_fetch_enabled`.
#[tauri::command]
pub fn set_check_for_updates_enabled(value: bool) -> config::AppConfig {
    let config = config::AppConfig {
        check_for_updates_enabled: value,
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// Records that the user dismissed the update banner for `version`, so it doesn't reappear on
/// the next launch for that same release — a newer release afterward isn't affected, since its
/// version won't match what's stored here.
#[tauri::command]
pub fn dismiss_update(version: String) -> config::AppConfig {
    let config = config::AppConfig {
        dismissed_update_version: Some(version),
        ..config::load_app_config()
    };
    config::save_app_config(&config);
    config
}

/// This repo's settings, e.g. whether commits
/// skip hooks by default. Never fails, same degrade-to-default behavior as `get_app_config`.
#[tauri::command]
pub fn get_repo_config(repo_path: String) -> config::RepoConfig {
    config::load_repo_config(Path::new(&repo_path))
}

/// Persists this repo's "skip hooks by default" preference, returning the resulting config.
#[tauri::command]
pub fn set_repo_default_skip_hooks(repo_path: String, value: bool) -> config::RepoConfig {
    let workdir = Path::new(&repo_path);
    let config = config::RepoConfig {
        default_skip_hooks: value,
    };
    config::save_repo_config(workdir, &config);
    config
}

/// The raw `commit.template` path as configured in this repo's `.git/config`, for a "This
/// Repository" settings field to display/edit — distinct from `commit_message_template`,
/// which resolves and reads the template file's contents for the commit-box prefill.
#[tauri::command]
pub fn get_commit_template_path(repo_path: String) -> PushGitResult<Option<String>> {
    stage::commit_template_path(&repo::open(Path::new(&repo_path))?)
}

/// Sets (`Some`) or clears (`None`) this repo's `commit.template` key directly in
/// `.git/config` — not app-owned storage, so PushGit and the real `git` CLI always agree.
#[tauri::command]
pub fn set_commit_template_path(repo_path: String, path: Option<String>) -> PushGitResult<()> {
    stage::set_commit_template_path(&repo::open(Path::new(&repo_path))?, path.as_deref())
}

/// The repo path passed on the command line at cold start (`tauri-plugin-cli`),
/// if any — `take()`n so a second
/// call returns `None` rather than re-opening the same repo forever. Pull-based (the
/// frontend calls this from `onMount`) rather than an emitted event, since an event fired
/// during `.setup()` can race the frontend's listener registration and be lost silently.
#[tauri::command]
pub fn get_startup_repo_path(state: State<'_, AppState>) -> Option<String> {
    state
        .startup_repo_path
        .lock()
        .expect("startup_repo_path mutex poisoned")
        .take()
}

/// This app's AI-assist settings (provider/model/instructions/cloud-warning-ack) — never
/// includes the API key itself, see `has_ai_api_key`.
#[tauri::command]
pub fn get_ai_settings() -> ai::AiSettings {
    config::load_app_config().ai
}

/// Persists the chosen AI transport (or clears it, `None`), returning the resulting settings.
/// Loads the existing config first so other fields aren't clobbered back to their defaults,
/// same reasoning as `set_max_commits_rendered`.
#[tauri::command]
pub fn set_ai_transport(transport: Option<ai::AiTransport>) -> ai::AiSettings {
    let mut config = config::load_app_config();
    config.ai.transport = transport;
    config::save_app_config(&config);
    config.ai
}

/// Persists the free-text instructions appended to every generation prompt.
#[tauri::command]
pub fn set_ai_instructions(instructions: String) -> ai::AiSettings {
    let mut config = config::load_app_config();
    config.ai.instructions = instructions;
    config::save_app_config(&config);
    config.ai
}

/// Records that the user has confirmed the one-time cloud-egress warning — called only from
/// that confirmation dialog's "Continue" action, never shown again once set.
#[tauri::command]
pub fn acknowledge_ai_cloud_warning() -> ai::AiSettings {
    let mut config = config::load_app_config();
    config.ai.cloud_warning_acknowledged = true;
    config::save_app_config(&config);
    config.ai
}

/// Saves the AI provider API key to the OS keyring — never written to `config.json`. Surfaces
/// keyring failures rather than swallowing them; see `ai::keys`'s module doc for why.
#[tauri::command]
pub fn set_ai_api_key(key: String) -> PushGitResult<()> {
    ai::store_api_key(&key)
}

/// Clears the stored AI provider API key, if any.
#[tauri::command]
pub fn clear_ai_api_key() -> PushGitResult<()> {
    ai::clear_api_key()
}

/// Existence check only — the frontend never reads the key's value back once saved, the same
/// "write-only secret field" pattern password managers use.
#[tauri::command]
pub fn has_ai_api_key() -> bool {
    ai::has_api_key()
}

/// Streams a generated commit message for `repo_path`'s staged diff through `channel`,
/// registering a fresh cancellation token first — `cancel_ai_generation` sets it. Uses its
/// own `ai_cancellation` registry, distinct from `remote_cancellation`, so a concurrent
/// fetch/pull/push on the same repo can't orphan this cancel token or vice versa.
#[tauri::command]
pub async fn generate_commit_message(
    repo_path: String,
    channel: Channel<ai::AiChunk>,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    let cancel = state.ai_cancellation.register(Path::new(&repo_path)).await;
    ai::generate_commit_message(
        Path::new(&repo_path),
        &channel,
        &cancel,
        &state.local_engine,
    )
    .await
}

/// Cancels whatever AI generation is currently in flight for `repo_path`, if any — a no-op
/// if nothing is (already finished, or never started).
#[tauri::command]
pub async fn cancel_ai_generation(
    repo_path: String,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    state.ai_cancellation.cancel(Path::new(&repo_path)).await;
    Ok(())
}

/// Whether the pinned local-AI model/engine are downloaded and verified for `engine_variant` —
/// checked on Settings load and again after a download completes. The Settings UI asks about
/// whichever variant is currently *drafted* (not yet saved), so previewing/downloading a
/// variant never requires clicking Save first.
#[tauri::command]
pub async fn get_local_ai_status(engine_variant: ai::EngineVariant) -> ai::local::LocalAiStatus {
    ai::local::get_local_ai_status(engine_variant).await
}

/// Downloads and verifies the pinned local-AI engine for `engine_variant` and the (shared)
/// model, registering a fresh cancellation token first — `cancel_local_ai_download` sets it.
/// Registering also doubles as the guard against two concurrent downloads racing on the same
/// `.part` files; the token is cleared once this call finishes, however it finishes, so a later
/// download isn't rejected by a token nothing will ever clear.
#[tauri::command]
pub async fn download_local_ai(
    engine_variant: ai::EngineVariant,
    channel: Channel<ai::local::DownloadProgress>,
    state: State<'_, AppState>,
) -> PushGitResult<()> {
    let cancel = state.local_ai_cancellation.register().await?;
    let result = ai::local::download_local_ai(engine_variant, &channel, &cancel).await;
    state.local_ai_cancellation.clear().await;
    result
}

/// Cancels whatever local-AI download is currently in progress, if any — a no-op if nothing is.
#[tauri::command]
pub async fn cancel_local_ai_download(state: State<'_, AppState>) -> PushGitResult<()> {
    state.local_ai_cancellation.cancel().await;
    Ok(())
}
