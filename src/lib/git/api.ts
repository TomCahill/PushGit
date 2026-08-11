// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Thin wrappers over `invoke()` for the Tauri commands in
// `src-tauri/src/commands/mod.rs`. Kept free of UI/state logic so components stay easy to
// test against `mockIPC`.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import type {
  AiChunk,
  AiSettings,
  AiTransport,
  AppConfig,
  BlameLine,
  BranchInfo,
  CherryPickOutcome,
  CommitGraphPage,
  ConflictSides,
  DownloadProgress,
  EngineVariant,
  FileDiff,
  FileHistoryEntry,
  FinishOutcome,
  GitVersionCheck,
  GraphFilter,
  HookOutputLine,
  Hunk,
  LocalAiStatus,
  MergeOutcome,
  OperationSummary,
  RebaseCommitSummary,
  RebaseOutcome,
  RebaseStep,
  RemoteProgress,
  RepoConfig,
  RepoHealth,
  RepoState,
  ResetMode,
  StashEntry,
  UndoRedoStatus,
  WorkflowBranchKind,
  WorkflowConfig,
} from "./types";

export function openRepository(path: string): Promise<string> {
  return invoke("open_repository", { path });
}

/**
 * Native OS folder picker; resolves `null` if the user cancels.
 *
 * In E2E builds (`VITE_E2E`), WebDriver can't drive a real native dialog, so a
 * WebDriver-injected `window.__e2eDialogPath__` is returned instead of showing one —
 * the existing pattern for test-only behavior that never ships in a
 * real build.
 */
export function pickRepositoryFolder(): Promise<string | null> {
  if (import.meta.env.VITE_E2E) {
    const e2eOverride = (window as unknown as { __e2eDialogPath__?: string | null })
      .__e2eDialogPath__;
    if (e2eOverride !== undefined) {
      return Promise.resolve(e2eOverride);
    }
  }
  return openFolderPicker({ directory: true, multiple: false, title: "Open Repository" });
}

export function graphOpen(repoPath: string, filter: GraphFilter): Promise<string> {
  return invoke("graph_open", { repoPath, filter });
}

export function graphPage(sessionId: string, pageSize: number): Promise<CommitGraphPage> {
  return invoke("graph_page", { sessionId, pageSize });
}

export function graphClose(sessionId: string): Promise<void> {
  return invoke("graph_close", { sessionId });
}

export function diffCommit(repoPath: string, commitOid: string): Promise<FileDiff[]> {
  return invoke("diff_commit", { repoPath, commitOid });
}

/** A commit's changes against the live working directory (untracked files excluded, staged
 *  deletes blended in — matches plain `git diff <rev>`, not `git status`). */
export function diffCommitToWorkdir(repoPath: string, commitOid: string): Promise<FileDiff[]> {
  return invoke("diff_commit_to_workdir", { repoPath, commitOid });
}

/** Per-line authorship for `path` as of HEAD. */
export function blameFile(repoPath: string, path: string): Promise<BlameLine[]> {
  return invoke("blame_file", { repoPath, path });
}

/** The commits that changed `path`, newest first. */
export function fileHistory(repoPath: string, path: string): Promise<FileHistoryEntry[]> {
  return invoke("file_history", { repoPath, path });
}

/** The diff between two arbitrary commits (e.g. a commit and `"HEAD"`). */
export function diffBetweenCommits(
  repoPath: string,
  fromOid: string,
  toOid: string,
): Promise<FileDiff[]> {
  return invoke("diff_between_commits", { repoPath, fromOid, toOid });
}

export function diffUnstaged(repoPath: string): Promise<FileDiff[]> {
  return invoke("diff_unstaged", { repoPath });
}

export function diffStaged(repoPath: string): Promise<FileDiff[]> {
  return invoke("diff_staged", { repoPath });
}

export function stageFile(repoPath: string, path: string): Promise<void> {
  return invoke("stage_file", { repoPath, path });
}

export function unstageFile(repoPath: string, path: string): Promise<void> {
  return invoke("unstage_file", { repoPath, path });
}

export function stageHunk(repoPath: string, path: string, hunk: Hunk): Promise<void> {
  return invoke("stage_hunk", { repoPath, path, hunk });
}

export function unstageHunk(repoPath: string, path: string, hunk: Hunk): Promise<void> {
  return invoke("unstage_hunk", { repoPath, path, hunk });
}

/** `lineIndices` are positions into `hunk.lines` — sub-hunk (line-level) staging, the
 *  smaller sibling of `stageHunk`. */
export function stageLines(
  repoPath: string,
  path: string,
  hunk: Hunk,
  lineIndices: number[],
): Promise<void> {
  return invoke("stage_lines", { repoPath, path, hunk, lineIndices });
}

export function unstageLines(
  repoPath: string,
  path: string,
  hunk: Hunk,
  lineIndices: number[],
): Promise<void> {
  return invoke("unstage_lines", { repoPath, path, hunk, lineIndices });
}

/** `skipHooks` bypasses `pre-commit`/`commit-msg` — real `git commit --no-verify`'s
 *  equivalent. `post-commit` always runs regardless. `onHookOutput`, if given, is called with
 *  each hook output line as it's produced (see `HookOutputModal`). */
export function commitChanges(
  repoPath: string,
  message: string,
  amend: boolean,
  skipHooks: boolean,
  onHookOutput?: (line: HookOutputLine) => void,
): Promise<string> {
  return invoke("commit", {
    repoPath,
    message,
    amend,
    skipHooks,
    hookOutput: hookOutputChannel(onHookOutput),
  });
}

export function headCommitMessage(repoPath: string): Promise<string | null> {
  return invoke("head_commit_message", { repoPath });
}

/** The `commit.template` file's content, if configured — `null` when unset or unreadable. */
export function commitMessageTemplate(repoPath: string): Promise<string | null> {
  return invoke("commit_message_template", { repoPath });
}

/** The raw `commit.template` path as configured in `.git/config`, for editing — `null` when
 *  unset. Distinct from `commitMessageTemplate` above, which resolves and reads the file's
 *  contents for the commit-box prefill. */
export function getCommitTemplatePath(repoPath: string): Promise<string | null> {
  return invoke("get_commit_template_path", { repoPath });
}

/** Sets (or, with `null`, clears) this repo's `commit.template` key directly in
 *  `.git/config` — not app-owned storage. */
export function setCommitTemplatePath(repoPath: string, path: string | null): Promise<void> {
  return invoke("set_commit_template_path", { repoPath, path });
}

export function listBranches(repoPath: string): Promise<BranchInfo[]> {
  return invoke("list_branches", { repoPath });
}

export function createBranch(repoPath: string, name: string, at?: string): Promise<void> {
  return invoke("create_branch", { repoPath, name, at: at ?? null });
}

export function checkoutBranch(repoPath: string, name: string): Promise<void> {
  return invoke("checkout_branch", { repoPath, name });
}

export function checkoutCommit(repoPath: string, oid: string): Promise<void> {
  return invoke("checkout_commit", { repoPath, oid });
}

export function checkoutRemoteBranch(repoPath: string, remoteBranchName: string): Promise<void> {
  return invoke("checkout_remote_branch", { repoPath, remoteBranchName });
}

export function deleteBranch(repoPath: string, name: string): Promise<void> {
  return invoke("delete_branch", { repoPath, name });
}

export function renameBranch(repoPath: string, oldName: string, newName: string): Promise<void> {
  return invoke("rename_branch", { repoPath, oldName, newName });
}

export function mergeBranch(repoPath: string, branchName: string): Promise<MergeOutcome> {
  return invoke("merge_branch", { repoPath, branchName });
}

export function abortMerge(repoPath: string): Promise<void> {
  return invoke("abort_merge", { repoPath });
}

/** Applies `commitOid` onto the current HEAD as a new commit, preserving its original
 *  author. A conflicted cherry-pick is finished later via the ordinary `commitChanges` —
 *  the backend detects `CHERRY_PICK_HEAD` itself, no separate "continue" call needed. */
export function cherryPickCommit(repoPath: string, commitOid: string): Promise<CherryPickOutcome> {
  return invoke("cherry_pick_commit", { repoPath, commitOid });
}

export function abortCherryPick(repoPath: string): Promise<void> {
  return invoke("abort_cherry_pick", { repoPath });
}

/** Whether a paused cherry-pick was started by `cherryPickRange` (a multi-commit
 *  sequence) rather than the single-commit `cherryPickCommit`. */
export function isMultiCherryPickInProgress(repoPath: string): Promise<boolean> {
  return invoke("is_multi_cherry_pick_in_progress", { repoPath });
}

/** Applies `commitOids`, in the given order, onto HEAD — stops at the first conflict.
 *  Resolve a conflict via `continueCherryPickRange`, not the ordinary commit box: real `git
 *  cherry-pick --continue` both commits the resolved step and advances the sequence. */
export function cherryPickRange(
  repoPath: string,
  commitOids: string[],
): Promise<CherryPickOutcome> {
  return invoke("cherry_pick_range", { repoPath, commitOids });
}

export function continueCherryPickRange(repoPath: string): Promise<CherryPickOutcome> {
  return invoke("continue_cherry_pick_range", { repoPath });
}

export function abortCherryPickRange(repoPath: string): Promise<void> {
  return invoke("abort_cherry_pick_range", { repoPath });
}

export function rebaseBranch(repoPath: string, onto: string): Promise<RebaseOutcome> {
  return invoke("rebase_branch", { repoPath, onto });
}

export function continueRebase(repoPath: string): Promise<RebaseOutcome> {
  return invoke("continue_rebase", { repoPath });
}

export function abortRebase(repoPath: string): Promise<void> {
  return invoke("abort_rebase", { repoPath });
}

/** `git reset --soft/--mixed/--hard <target>` — the commit graph's drag-to-reset gesture. */
export function resetTo(repoPath: string, target: string, mode: ResetMode): Promise<void> {
  return invoke("reset_to", { repoPath, target, mode });
}

export function listConflicts(repoPath: string): Promise<string[]> {
  return invoke("list_conflicts", { repoPath });
}

export function repositoryState(repoPath: string): Promise<RepoState> {
  return invoke("repository_state", { repoPath });
}

export function conflictSides(repoPath: string, path: string): Promise<ConflictSides> {
  return invoke("conflict_sides", { repoPath, path });
}

export function writeResolvedConflict(
  repoPath: string,
  path: string,
  content: string,
): Promise<void> {
  return invoke("write_resolved_conflict", { repoPath, path, content });
}

export function resolveConflictAsDeleted(repoPath: string, path: string): Promise<void> {
  return invoke("resolve_conflict_as_deleted", { repoPath, path });
}

/** Discards a file's uncommitted changes (staged and unstaged) back to HEAD, or removes it
 *  entirely if HEAD has no record of it. Unrecoverable outside the undo/redo stack. */
export function discardFileChanges(repoPath: string, path: string): Promise<void> {
  return invoke("discard_file_changes", { repoPath, path });
}

/** Undoes the most recently recorded destructive operation. Rejects if there's nothing to
 *  undo. */
export function undoLastOperation(repoPath: string): Promise<OperationSummary> {
  return invoke("undo_last_operation", { repoPath });
}

/** Re-applies the most recently undone operation. Rejects if there's nothing to redo. */
export function redoLastOperation(repoPath: string): Promise<OperationSummary> {
  return invoke("redo_last_operation", { repoPath });
}

export function undoRedoStatus(repoPath: string): Promise<UndoRedoStatus> {
  return invoke("undo_redo_status", { repoPath });
}

/** The commits between `onto` and HEAD, oldest first — the interactive rebase editor's
 *  starting list, before the user reorders/edits anything. */
export function listRebaseCommits(repoPath: string, onto: string): Promise<RebaseCommitSummary[]> {
  return invoke("list_rebase_commits", { repoPath, onto });
}

/** Whether a paused rebase (`repositoryState() === "rebase"`) was started by the
 *  interactive rebase editor rather than plain `rebaseBranch` — `BranchSidebar` uses this
 *  to suppress its own conflict banner (whose Continue/Abort call the wrong backend) for a
 *  rebase only the interactive editor should resolve. */
export function isInteractiveRebaseInProgress(repoPath: string): Promise<boolean> {
  return invoke("is_interactive_rebase_in_progress", { repoPath });
}

export function startInteractiveRebase(
  repoPath: string,
  onto: string,
  steps: RebaseStep[],
): Promise<RebaseOutcome> {
  return invoke("start_interactive_rebase", { repoPath, onto, steps });
}

export function continueInteractiveRebase(repoPath: string): Promise<RebaseOutcome> {
  return invoke("continue_interactive_rebase", { repoPath });
}

export function abortInteractiveRebase(repoPath: string): Promise<void> {
  return invoke("abort_interactive_rebase", { repoPath });
}

export function listTags(repoPath: string): Promise<string[]> {
  return invoke("list_tags", { repoPath });
}

export function createTag(
  repoPath: string,
  name: string,
  at?: string,
  message?: string,
): Promise<void> {
  return invoke("create_tag", { repoPath, name, at: at ?? null, message: message ?? null });
}

export function deleteTag(repoPath: string, name: string): Promise<void> {
  return invoke("delete_tag", { repoPath, name });
}

/** Force-moves an existing tag to `to` (any commit-ish) — the graph's drag-a-tag-onto-a-
 *  commit-or-branch-badge gesture. */
export function moveTag(repoPath: string, name: string, to: string): Promise<void> {
  return invoke("move_tag", { repoPath, name, to });
}

export function renameTag(repoPath: string, oldName: string, newName: string): Promise<void> {
  return invoke("rename_tag", { repoPath, oldName, newName });
}

export function detectWorkflow(repoPath: string): Promise<WorkflowConfig | null> {
  return invoke("detect_workflow", { repoPath });
}

export function initWorkflow(repoPath: string, config: WorkflowConfig): Promise<void> {
  return invoke("init_workflow", { repoPath, config });
}

export function startWorkflowBranch(
  repoPath: string,
  config: WorkflowConfig,
  kind: WorkflowBranchKind,
  name: string,
): Promise<void> {
  return invoke("start_workflow_branch", { repoPath, config, kind, name });
}

export function finishWorkflowBranch(
  repoPath: string,
  config: WorkflowConfig,
  kind: WorkflowBranchKind,
  name: string,
): Promise<FinishOutcome> {
  return invoke("finish_workflow_branch", { repoPath, config, kind, name });
}

export function createStash(repoPath: string, message?: string): Promise<string> {
  return invoke("create_stash", { repoPath, message: message ?? null });
}

/** Stashes only `paths`' changes, leaving every other file's uncommitted changes as-is. */
export function createStashForPaths(
  repoPath: string,
  paths: string[],
  message?: string,
): Promise<string> {
  return invoke("create_stash_for_paths", { repoPath, message: message ?? null, paths });
}

/** Renames a stash by rewriting its reflog message. */
export function renameStash(repoPath: string, index: number, newMessage: string): Promise<void> {
  return invoke("rename_stash", { repoPath, index, newMessage });
}

export function listStashes(repoPath: string): Promise<StashEntry[]> {
  return invoke("list_stashes", { repoPath });
}

export function applyStash(repoPath: string, index: number): Promise<void> {
  return invoke("apply_stash", { repoPath, index });
}

export function popStash(repoPath: string, index: number): Promise<void> {
  return invoke("pop_stash", { repoPath, index });
}

export function dropStash(repoPath: string, index: number): Promise<void> {
  return invoke("drop_stash", { repoPath, index });
}

export function checkGitVersion(): Promise<GitVersionCheck> {
  return invoke("check_git_version");
}

/** A rough repo-health snapshot (loose object/pack counts and sizes) from `git
 *  count-objects -v`. */
export function repoHealth(repoPath: string): Promise<RepoHealth> {
  return invoke("repo_health", { repoPath });
}

export function runGc(repoPath: string): Promise<void> {
  return invoke("run_gc", { repoPath });
}

function progressChannel(onProgress?: (progress: RemoteProgress) => void): Channel<RemoteProgress> {
  const channel = new Channel<RemoteProgress>();
  if (onProgress) {
    channel.onmessage = onProgress;
  }
  return channel;
}

function hookOutputChannel(onHookOutput?: (line: HookOutputLine) => void): Channel<HookOutputLine> {
  const channel = new Channel<HookOutputLine>();
  if (onHookOutput) {
    channel.onmessage = onHookOutput;
  }
  return channel;
}

export function fetchRemote(
  repoPath: string,
  remoteName: string,
  onProgress?: (progress: RemoteProgress) => void,
): Promise<void> {
  return invoke("fetch", { repoPath, remoteName, progress: progressChannel(onProgress) });
}

export function pullRemote(
  repoPath: string,
  remoteName: string,
  onProgress?: (progress: RemoteProgress) => void,
): Promise<MergeOutcome> {
  return invoke("pull", { repoPath, remoteName, progress: progressChannel(onProgress) });
}

export function pushRemote(
  repoPath: string,
  remoteName: string,
  branchName: string,
  force: boolean,
  onProgress?: (progress: RemoteProgress) => void,
  onHookOutput?: (line: HookOutputLine) => void,
): Promise<void> {
  return invoke("push", {
    repoPath,
    remoteName,
    branchName,
    force,
    progress: progressChannel(onProgress),
    hookOutput: hookOutputChannel(onHookOutput),
  });
}

export function cancelRemoteOperation(repoPath: string): Promise<void> {
  return invoke("cancel_remote_operation", { repoPath });
}

export function startRepoWatcher(repoPath: string): Promise<void> {
  return invoke("start_repo_watcher", { repoPath });
}

export function stopRepoWatcher(): Promise<void> {
  return invoke("stop_repo_watcher");
}

/** Fires whenever the backend's filesystem watcher observes a relevant repo change. */
export function onRepoChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("repo-changed", () => callback());
}

/** A native menu item was clicked.
 *  `callback` receives the clicked item's id, e.g. `"open-repository"`. */
export function onMenuAction(callback: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("menu-action", (event) => callback(event.payload));
}

/** App-wide settings. Never rejects — a
 *  missing/corrupt config file degrades to defaults on the backend. */
export function getAppConfig(): Promise<AppConfig> {
  return invoke("get_app_config");
}

/** Clamps and persists a new max-commits-rendered default, returning the resulting config. */
export function setMaxCommitsRendered(value: number): Promise<AppConfig> {
  return invoke("set_max_commits_rendered", { value });
}

/** Persists the app-wide "reduce motion" preference, returning the resulting config. */
export function setReduceMotion(value: boolean): Promise<AppConfig> {
  return invoke("set_reduce_motion", { value });
}

/** Persists whether the periodic auto-fetch timer is enabled, returning the resulting config. */
export function setAutoFetchEnabled(value: boolean): Promise<AppConfig> {
  return invoke("set_auto_fetch_enabled", { value });
}

/** Clamps and persists the auto-fetch interval in minutes, returning the resulting config. */
export function setAutoFetchIntervalMinutes(value: number): Promise<AppConfig> {
  return invoke("set_auto_fetch_interval_minutes", { value });
}

/** Persists whether `HookOutputModal` opens immediately when a commit/push hook starts
 *  running, versus staying hidden until the operation fails, returning the resulting config. */
export function setShowHookOutputAlways(value: boolean): Promise<AppConfig> {
  return invoke("set_show_hook_output_always", { value });
}

/** This repo's settings. Never rejects, same
 *  degrade-to-default behavior as `getAppConfig`. */
export function getRepoConfig(repoPath: string): Promise<RepoConfig> {
  return invoke("get_repo_config", { repoPath });
}

/** Persists this repo's "skip hooks by default" preference, returning the resulting config. */
export function setRepoDefaultSkipHooks(repoPath: string, value: boolean): Promise<RepoConfig> {
  return invoke("set_repo_default_skip_hooks", { repoPath, value });
}

/** The repo path passed on the command line at cold start (`tauri-plugin-cli`),
 *  if any. `null` on every call after
 *  the first — the backend `take()`s it so a page reload doesn't keep re-opening the same repo. */
export function getStartupRepoPath(): Promise<string | null> {
  return invoke("get_startup_repo_path");
}

/** Persists the chosen AI transport (`null` clears it), returning the resulting settings. */
export function setAiTransport(transport: AiTransport | null): Promise<AiSettings> {
  return invoke("set_ai_transport", { transport });
}

/** Persists the free-text instructions appended to every generation prompt. */
export function setAiInstructions(instructions: string): Promise<AiSettings> {
  return invoke("set_ai_instructions", { instructions });
}

/** Records that the user has confirmed the one-time cloud-egress warning — never shown again
 *  once set. */
export function acknowledgeAiCloudWarning(): Promise<AiSettings> {
  return invoke("acknowledge_ai_cloud_warning");
}

/** Saves the AI provider API key to the OS keyring. */
export function setAiApiKey(key: string): Promise<void> {
  return invoke("set_ai_api_key", { key });
}

/** Clears the stored AI provider API key, if any. */
export function clearAiApiKey(): Promise<void> {
  return invoke("clear_ai_api_key");
}

/** Existence check only — the key's value is never read back once saved. */
export function hasAiApiKey(): Promise<boolean> {
  return invoke("has_ai_api_key");
}

/** Streams a generated commit message for `repoPath`'s staged diff, calling `onChunk` with
 *  each raw text delta as it arrives. */
export function generateCommitMessage(
  repoPath: string,
  onChunk: (text: string) => void,
): Promise<void> {
  const channel = new Channel<AiChunk>();
  channel.onmessage = (chunk) => onChunk(chunk.text);
  return invoke("generate_commit_message", { repoPath, channel });
}

/** Cancels whatever AI generation is currently in flight for `repoPath`, if any. */
export function cancelAiGeneration(repoPath: string): Promise<void> {
  return invoke("cancel_ai_generation", { repoPath });
}

/** Whether the pinned local-AI model/engine are downloaded and verified for `engineVariant`. */
export function getLocalAiStatus(engineVariant: EngineVariant): Promise<LocalAiStatus> {
  return invoke("get_local_ai_status", { engineVariant });
}

/** Downloads and verifies the pinned local-AI engine for `engineVariant` and the (shared)
 *  model, calling `onProgress` with each update as it arrives. */
export function downloadLocalAi(
  engineVariant: EngineVariant,
  onProgress: (progress: DownloadProgress) => void,
): Promise<void> {
  const channel = new Channel<DownloadProgress>();
  channel.onmessage = onProgress;
  return invoke("download_local_ai", { engineVariant, channel });
}

/** Cancels whatever local-AI download is currently in progress, if any. */
export function cancelLocalAiDownload(): Promise<void> {
  return invoke("cancel_local_ai_download");
}
