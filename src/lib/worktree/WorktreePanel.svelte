<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Worktree panel: lists the main working directory plus every linked worktree, with
  // Open/Remove per row, and a form to add a new one (existing local/remote branch, or a
  // brand new branch). "Open" reuses whatever the caller already does to switch repos
  // (`onOpen`, wired to `+page.svelte`'s `openRepo`) — a worktree's directory is an ordinary
  // repository, so there's no separate "enter a worktree" concept to build.
  import {
    addWorktree,
    listBranches,
    listRemoteBranches,
    listWorktrees,
    pickWorktreeParentFolder,
    removeWorktree,
  } from "$lib/git/api";
  import { confirmAsync } from "$lib/shell/confirmDialog.svelte";
  import { createReloadable } from "$lib/shell/reloadable.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type { WorktreeInfo } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onCountChange,
    onOpen,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onCountChange?: (count: number) => void;
    /** Switches the app into the worktree at this path — exactly what every other "open a
     *  different repo" action already does. */
    onOpen: (path: string) => void;
  } = $props();

  let worktrees = $state<WorktreeInfo[]>([]);
  let loadError = $state<string | null>(null);
  let branchOptions = $state<string[]>([]);

  let newBranch = $state("");
  let newStartPoint = $state("");
  let newLocation = $state("");
  let locationEdited = $state(false);

  const worktreeReload = createReloadable(async (path, isStale) => {
    if (!path) {
      worktrees = [];
      onCountChange?.(0);
      return;
    }

    loadError = null;
    try {
      const list = await listWorktrees(path);
      if (isStale()) return;
      worktrees = list;
      onCountChange?.(Math.max(0, list.length - 1));
    } catch (err) {
      if (!isStale()) {
        loadError = String(err);
        notifyError(loadError);
      }
    }
  });

  let busy = $derived(worktreeReload.busy);

  $effect(() => {
    void worktreeReload.reload(repoPath, refreshKey);
  });

  // Autocomplete only — the picker doesn't restrict `newBranch` to this list, matching
  // `CompareRefsPicker`'s own free-text-with-suggestions precedent.
  $effect(() => {
    const path = repoPath;
    let cancelled = false;
    void (async () => {
      try {
        const [branches, remoteBranches] = await Promise.all([
          listBranches(path),
          listRemoteBranches(path),
        ]);
        if (cancelled) return;
        branchOptions = [...branches.map((b) => b.name), ...remoteBranches];
      } catch {
        // Autocomplete is a convenience only — leave the field usable for manual entry.
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  function sanitizedLeaf(branch: string): string {
    return branch.trim().replace(/\//g, "-") || "worktree";
  }

  function splitDir(path: string): { parent: string; leaf: string } {
    const trimmed = path.replace(/[/\\]+$/, "");
    const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
    return idx === -1
      ? { parent: trimmed, leaf: trimmed }
      : { parent: trimmed.slice(0, idx), leaf: trimmed.slice(idx + 1) };
  }

  /** Sibling of the repo itself, grouped under one `<repo>-worktrees` folder rather than
   *  scattered loose directories next to it — just a pre-fill suggestion, fully editable. */
  function defaultLocation(branch: string): string {
    if (!repoPath) return "";
    const { parent, leaf } = splitDir(repoPath);
    return `${parent}/${leaf}-worktrees/${sanitizedLeaf(branch)}`;
  }

  $effect(() => {
    if (!locationEdited) {
      newLocation = defaultLocation(newBranch);
    }
  });

  function handleLocationInput(event: Event & { currentTarget: HTMLInputElement }) {
    newLocation = event.currentTarget.value;
    locationEdited = true;
  }

  async function handleBrowse() {
    const picked = await pickWorktreeParentFolder();
    if (!picked) return;
    newLocation = `${picked}/${sanitizedLeaf(newBranch)}`;
    locationEdited = true;
  }

  function runAction(fn: () => Promise<void>) {
    void worktreeReload.runAction(repoPath, fn, notifyError, onChanged);
  }

  function handleCreateSubmit(event: SubmitEvent) {
    event.preventDefault();
    const branch = newBranch.trim();
    const location = newLocation.trim();
    const startPoint = newStartPoint.trim() || undefined;
    if (!branch || !location || busy) return;
    runAction(async () => {
      await addWorktree(repoPath, branch, startPoint, location);
      notifySuccess(`Created worktree for "${branch}".`);
      newBranch = "";
      newStartPoint = "";
      newLocation = "";
      locationEdited = false;
      onOpen(location);
    });
  }

  function isCurrent(entry: WorktreeInfo): boolean {
    return repoPath !== "" && entry.path === repoPath;
  }

  function handleOpen(entry: WorktreeInfo) {
    onOpen(entry.path);
  }

  async function handleRemove(entry: WorktreeInfo) {
    const label = entry.branch ?? entry.name;
    const message = entry.isDirty
      ? `"${label}" has uncommitted changes that will be lost. Remove this worktree anyway?`
      : `Remove worktree "${label}"? The folder at ${entry.path} will be deleted.`;
    if (!(await confirmAsync(message))) return;
    runAction(async () => {
      await removeWorktree(repoPath, entry.name);
      notifySuccess(`Removed worktree "${label}".`);
    });
  }
</script>

<div class="worktree-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  <ul class="worktree-list">
    {#each worktrees as entry (entry.name)}
      <li>
        <div class="worktree-row">
          <span class="worktree-branch" title={entry.path}>{entry.branch ?? entry.name}</span>
          {#if isCurrent(entry)}
            <span class="badge current">Current</span>
          {/if}
          {#if entry.isMissing}
            <span class="badge missing">Missing</span>
          {/if}
          {#if entry.isDirty}
            <span class="badge dirty">Uncommitted changes</span>
          {/if}
        </div>
        <div class="worktree-path" title={entry.path}>{entry.path}</div>
        <div class="worktree-actions">
          <button
            type="button"
            onclick={() => handleOpen(entry)}
            disabled={busy || isCurrent(entry) || entry.isMissing}
          >
            Open
          </button>
          {#if !entry.isMain}
            <button
              type="button"
              onclick={() => handleRemove(entry)}
              disabled={busy || isCurrent(entry)}
              title={isCurrent(entry)
                ? "Switch to a different repo or worktree before removing this one"
                : `Remove "${entry.branch ?? entry.name}"`}
            >
              Remove
            </button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  <form class="create-worktree" onsubmit={handleCreateSubmit}>
    <label for="wt-branch">Branch</label>
    <input
      id="wt-branch"
      type="text"
      bind:value={newBranch}
      list="worktree-branch-options"
      placeholder="e.g. feature-x"
    />
    <datalist id="worktree-branch-options">
      {#each branchOptions as name (name)}
        <option value={name}></option>
      {/each}
    </datalist>

    <label for="wt-start-point">
      Start point <span class="hint">(used only when creating a new branch)</span>
    </label>
    <input id="wt-start-point" type="text" bind:value={newStartPoint} placeholder="HEAD" />

    <label for="wt-location">Location</label>
    <div class="location-row">
      <input id="wt-location" type="text" value={newLocation} oninput={handleLocationInput} />
      <button type="button" onclick={handleBrowse} disabled={busy}>Browse…</button>
    </div>

    <button type="submit" disabled={busy || !newBranch.trim() || !newLocation.trim()}>
      Create Worktree
    </button>
  </form>
</div>

<style>
  .worktree-panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 22rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .worktree-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    max-height: 16rem;
    overflow-y: auto;
  }

  .worktree-list li {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.3rem 0.35rem;
    border-radius: var(--radius-sm);
  }

  .worktree-list li:hover {
    background: var(--surface-2);
  }

  .worktree-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
  }

  .worktree-branch {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .badge {
    flex-shrink: 0;
    font-size: 0.65rem;
    padding: 0.05rem 0.4rem;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    background: var(--surface-2);
  }

  .badge.current {
    color: var(--accent);
    border-color: var(--accent);
  }

  .badge.dirty {
    color: var(--warning);
    border-color: var(--warning);
  }

  .badge.missing {
    color: var(--danger);
    border-color: var(--danger);
  }

  .worktree-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .worktree-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .worktree-actions button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .worktree-actions button:not(:disabled):hover {
    background: var(--surface-2);
  }

  .create-worktree {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .create-worktree label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .hint {
    font-weight: normal;
    opacity: 0.8;
  }

  .create-worktree input {
    font: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.5rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .create-worktree input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .location-row {
    display: flex;
    gap: 0.35rem;
  }

  .location-row input {
    flex: 1 1 auto;
    min-width: 0;
  }

  .location-row button,
  .create-worktree > button[type="submit"] {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .location-row button:not(:disabled):hover,
  .create-worktree > button[type="submit"]:not(:disabled):hover {
    background: var(--surface-2);
  }
</style>
