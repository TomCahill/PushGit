<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Branch sidebar: local branch list with checkout/create/delete/rename, merge/rebase
  // actions against the current HEAD, and conflict recovery (abort/continue) driven by the
  // repository's actual on-disk state rather than in-memory guesses — so reopening a repo
  // that's already mid-conflict from outside PushGit still shows the right controls.
  // Conflicted files themselves are resolved via the existing working-directory staging
  // view: fix the markers in your own editor, then stage as usual —
  // `onConflicts` tells the parent to switch to that view. Rename is available on the
  // current (HEAD) branch too, unlike merge/rebase/delete — libgit2's branch rename updates
  // HEAD's symbolic ref automatically when it points at the renamed branch.
  import {
    abortCherryPick,
    abortCherryPickRange,
    abortMerge,
    abortRebase,
    checkoutBranch,
    continueCherryPickRange,
    continueRebase,
    createBranch,
    deleteBranch,
    isInteractiveRebaseInProgress,
    isMultiCherryPickInProgress,
    listBranches,
    listConflicts,
    listRebaseCommits,
    mergeBranch,
    rebaseBranch,
    renameBranch,
    repositoryState,
  } from "$lib/git/api";
  import { confirmAsync, promptAsync } from "$lib/shell/confirmDialog.svelte";
  import CopyButton from "$lib/shell/CopyButton.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type {
    BranchInfo,
    MergeOutcome,
    RebaseCommitSummary,
    RebaseOutcome,
    RepoState,
  } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onConflicts,
    onCurrentBranchChange,
    onInteractiveRebase,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onConflicts?: () => void;
    /** Called whenever the HEAD branch name changes (including to `null` while loading or
     *  in a detached-HEAD state) — lets a toolbar trigger show it without re-fetching. */
    onCurrentBranchChange?: (name: string | null) => void;
    /** Called with the commits between `onto` and HEAD once fetched — the parent switches to
     *  the interactive rebase editor view with them. */
    onInteractiveRebase?: (onto: string, commits: RebaseCommitSummary[]) => void;
  } = $props();

  let branches = $state<BranchInfo[]>([]);
  let repoState = $state<RepoState>("clean");
  let conflicts = $state<string[]>([]);
  let interactiveRebaseInProgress = $state(false);
  let multiCherryPickInProgress = $state(false);
  let loadError = $state<string | null>(null);

  let busy = $state(false);

  let newBranchName = $state("");

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      branches = [];
      repoState = "clean";
      conflicts = [];
      onCurrentBranchChange?.(null);
      return;
    }

    loadError = null;
    try {
      const [branchList, state, conflictList] = await Promise.all([
        listBranches(path),
        repositoryState(path),
        listConflicts(path),
      ]);
      const isInteractive = state === "rebase" ? await isInteractiveRebaseInProgress(path) : false;
      const isMultiCherryPick =
        state === "cherry_pick" ? await isMultiCherryPickInProgress(path) : false;
      if (myGeneration !== generation) return;
      branches = branchList;
      repoState = state;
      conflicts = conflictList;
      interactiveRebaseInProgress = isInteractive;
      multiCherryPickInProgress = isMultiCherryPick;
      onCurrentBranchChange?.(branchList.find((b) => b.isHead)?.name ?? null);
    } catch (err) {
      if (myGeneration === generation) {
        loadError = String(err);
        notifyError(loadError);
      }
    }
  }

  async function runAction(fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await fn();
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      notifyError(String(err));
    } finally {
      busy = false;
    }
  }

  function describeMergeOutcome(branchName: string, outcome: MergeOutcome): string {
    switch (outcome.kind) {
      case "fast_forward":
        return `Fast-forwarded to ${branchName}.`;
      case "already_up_to_date":
        return "Already up to date.";
      case "merged":
        return `Merged ${branchName}.`;
      case "conflicts":
        return `Merge stopped with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function describeRebaseOutcome(outcome: RebaseOutcome): string {
    switch (outcome.kind) {
      case "completed":
        return "Rebase completed.";
      case "conflicts":
        return `Rebase paused with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function handleCheckout(branch: BranchInfo) {
    if (branch.isHead) return;
    void runAction(async () => {
      await checkoutBranch(repoPath, branch.name);
      notifySuccess(`Checked out ${branch.name}.`);
    });
  }

  function handleCreateSubmit(event: SubmitEvent) {
    event.preventDefault();
    const name = newBranchName.trim();
    if (!name) return;
    void runAction(async () => {
      await createBranch(repoPath, name);
      newBranchName = "";
      notifySuccess(`Created branch "${name}".`);
    });
  }

  async function handleDelete(branch: BranchInfo, event: MouseEvent) {
    event.stopPropagation();
    if (!(await confirmAsync(`Delete branch "${branch.name}"?`))) return;
    void runAction(async () => {
      await deleteBranch(repoPath, branch.name);
      notifySuccess(`Deleted branch "${branch.name}".`);
    });
  }

  async function handleRename(branch: BranchInfo, event: MouseEvent) {
    event.stopPropagation();
    const newName = (await promptAsync(`Rename branch "${branch.name}" to:`, branch.name))?.trim();
    if (!newName || newName === branch.name) return;
    void runAction(async () => {
      await renameBranch(repoPath, branch.name, newName);
      notifySuccess(`Renamed "${branch.name}" to "${newName}".`);
    });
  }

  function handleMerge(branch: BranchInfo, event: MouseEvent) {
    event.stopPropagation();
    void runAction(async () => {
      const outcome = await mergeBranch(repoPath, branch.name);
      notifySuccess(describeMergeOutcome(branch.name, outcome));
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleRebase(branch: BranchInfo, event: MouseEvent) {
    event.stopPropagation();
    void runAction(async () => {
      const outcome = await rebaseBranch(repoPath, branch.name);
      notifySuccess(describeRebaseOutcome(outcome));
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleInteractiveRebase(branch: BranchInfo, event: MouseEvent) {
    event.stopPropagation();
    void runAction(async () => {
      const commits = await listRebaseCommits(repoPath, branch.name);
      onInteractiveRebase?.(branch.name, commits);
    });
  }

  function handleContinueRebase() {
    void runAction(async () => {
      const outcome = await continueRebase(repoPath);
      notifySuccess(describeRebaseOutcome(outcome));
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleAbortMerge() {
    void runAction(async () => {
      await abortMerge(repoPath);
      notifySuccess("Merge aborted.");
    });
  }

  function handleAbortCherryPick() {
    void runAction(async () => {
      await abortCherryPick(repoPath);
      notifySuccess("Cherry-pick aborted.");
    });
  }

  function handleContinueCherryPickRange() {
    void runAction(async () => {
      const outcome = await continueCherryPickRange(repoPath);
      notifySuccess(
        outcome.kind === "conflicts"
          ? `Cherry-pick stopped with ${outcome.conflicts.length} conflicting file(s).`
          : "Cherry-pick completed.",
      );
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleAbortCherryPickRange() {
    void runAction(async () => {
      await abortCherryPickRange(repoPath);
      notifySuccess("Cherry-pick range aborted.");
    });
  }

  function handleAbortRebase() {
    void runAction(async () => {
      await abortRebase(repoPath);
      notifySuccess("Rebase aborted.");
    });
  }
</script>

<div class="branch-sidebar">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  {#if repoState === "merge"}
    <div class="conflict-banner" role="alert">
      <p>
        Merge in progress — {conflicts.length} conflicting file(s). Resolve them in the working directory
        view, then commit to finish.
      </p>
      <button type="button" onclick={handleAbortMerge} disabled={busy}>Abort merge</button>
    </div>
  {:else if repoState === "rebase" && interactiveRebaseInProgress}
    <div class="conflict-banner" role="alert">
      <p>
        An interactive rebase is paused — {conflicts.length} conflicting file(s). Use the interactive
        rebase editor's own Continue/Abort to resolve it.
      </p>
    </div>
  {:else if repoState === "rebase"}
    <div class="conflict-banner" role="alert">
      <p>
        Rebase paused — {conflicts.length} conflicting file(s). Resolve them in the working directory
        view, then continue.
      </p>
      <div class="conflict-actions">
        <button type="button" onclick={handleContinueRebase} disabled={busy}>Continue rebase</button
        >
        <button type="button" onclick={handleAbortRebase} disabled={busy}>Abort rebase</button>
      </div>
    </div>
  {:else if repoState === "cherry_pick" && multiCherryPickInProgress}
    <div class="conflict-banner" role="alert">
      <p>
        Cherry-pick range paused — {conflicts.length} conflicting file(s). Resolve them in the working
        directory view, then continue.
      </p>
      <div class="conflict-actions">
        <button type="button" onclick={handleContinueCherryPickRange} disabled={busy}
          >Continue cherry-pick</button
        >
        <button type="button" onclick={handleAbortCherryPickRange} disabled={busy}
          >Abort cherry-pick</button
        >
      </div>
    </div>
  {:else if repoState === "cherry_pick"}
    <div class="conflict-banner" role="alert">
      <p>
        Cherry-pick paused — {conflicts.length} conflicting file(s). Resolve them in the working directory
        view, then commit to finish.
      </p>
      <button type="button" onclick={handleAbortCherryPick} disabled={busy}
        >Abort cherry-pick</button
      >
    </div>
  {:else if repoState === "other"}
    <div class="conflict-banner" role="alert">
      <p>An operation PushGit doesn't manage yet is in progress — resolve it with the git CLI.</p>
    </div>
  {/if}

  {#if branches.length === 0}
    <p class="placeholder">No branches.</p>
  {:else}
    <ul class="branch-list">
      {#each branches as branch (branch.name)}
        <li class:head={branch.isHead}>
          <button
            type="button"
            class="branch-row"
            onclick={() => handleCheckout(branch)}
            disabled={busy || branch.isHead}
            title={branch.isHead ? undefined : `Checkout ${branch.name}`}
          >
            <span class="branch-name">{branch.name}</span>
            {#if branch.isHead}
              <span class="head-badge">HEAD</span>
            {/if}
            {#if branch.upstream && (branch.ahead || branch.behind)}
              <span class="ahead-behind">↑{branch.ahead} ↓{branch.behind}</span>
            {/if}
          </button>
          <CopyButton text={branch.name} label={`Copy branch name ${branch.name}`} />
          <div class="branch-actions">
            <button
              type="button"
              title={`Rename ${branch.name}`}
              onclick={(event) => handleRename(branch, event)}
              disabled={busy}
            >
              Rename
            </button>
            {#if !branch.isHead}
              <button
                type="button"
                title={`Merge ${branch.name} into the current branch`}
                onclick={(event) => handleMerge(branch, event)}
                disabled={busy}
              >
                Merge
              </button>
              <button
                type="button"
                title={`Rebase the current branch onto ${branch.name}`}
                onclick={(event) => handleRebase(branch, event)}
                disabled={busy}
              >
                Rebase
              </button>
              {#if onInteractiveRebase}
                <button
                  type="button"
                  title={`Interactively rebase the current branch onto ${branch.name}`}
                  onclick={(event) => handleInteractiveRebase(branch, event)}
                  disabled={busy}
                >
                  Rebase (interactive)
                </button>
              {/if}
              <button
                type="button"
                title={`Delete ${branch.name}`}
                onclick={(event) => handleDelete(branch, event)}
                disabled={busy}
              >
                Delete
              </button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <form class="create-branch" onsubmit={handleCreateSubmit}>
    <label for="new-branch-name">New branch</label>
    <div class="create-branch-row">
      <input
        id="new-branch-name"
        type="text"
        bind:value={newBranchName}
        placeholder="branch name"
      />
      <button type="submit" disabled={busy || !newBranchName.trim()}>Create</button>
    </div>
  </form>
</div>

<style>
  .branch-sidebar {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .placeholder {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .conflict-banner {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.5rem;
    font-size: 0.8rem;
    background: var(--warning-bg);
    border: 1px solid var(--warning);
    border-radius: var(--radius-md);
  }

  .conflict-banner p {
    margin: 0;
  }

  .conflict-actions {
    display: flex;
    gap: 0.35rem;
  }

  .branch-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .branch-list li {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .branch-list li.head .branch-row {
    font-weight: 600;
  }

  .branch-row {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
    background: none;
    border: none;
    padding: 0.3rem 0.4rem;
    font: inherit;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: background-color 0.1s ease;
  }

  .branch-row:not(:disabled):hover {
    background: var(--accent-bg);
  }

  .branch-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
  }

  .head-badge {
    flex-shrink: 0;
    font-size: 0.65rem;
    padding: 0.05rem 0.3rem;
    border-radius: var(--radius-sm);
    background: var(--accent-bg);
    color: var(--accent);
  }

  .ahead-behind {
    flex-shrink: 0;
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .branch-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .branch-actions button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .branch-actions button:hover {
    background: var(--surface-2);
  }

  .create-branch {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .create-branch label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .create-branch-row {
    display: flex;
    gap: 0.35rem;
  }

  .create-branch-row input {
    flex: 1 1 auto;
    min-width: 0;
    font: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.5rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .create-branch-row input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .create-branch-row button {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .create-branch-row button:not(:disabled):hover {
    background: var(--surface-2);
  }
</style>
