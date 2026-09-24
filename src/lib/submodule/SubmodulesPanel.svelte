<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  import {
    cancelSubmoduleUpdate,
    initSubmodule,
    listSubmodules,
    syncSubmodule,
    updateSubmodule,
  } from "$lib/git/api";
  import { createReloadable } from "$lib/shell/reloadable.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type { RemoteProgress, SubmoduleInfo } from "$lib/git/types";

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
    onOpen: (path: string) => void;
  } = $props();

  let submodules = $state<SubmoduleInfo[]>([]);
  let loadError = $state<string | null>(null);
  let recursive = $state(false);

  // Update streams progress and is cancellable, which createReloadable.runAction can't express.
  let updateBusy = $state(false);
  let updateProgress = $state<RemoteProgress | null>(null);

  const submoduleReload = createReloadable(async (path, isStale) => {
    if (!path) {
      submodules = [];
      onCountChange?.(0);
      return;
    }

    loadError = null;
    try {
      const list = await listSubmodules(path);
      if (isStale()) return;
      submodules = list;
      onCountChange?.(
        list.filter((s) => !s.isInitialized || s.isMissing || s.needsUpdate).length,
      );
    } catch (err) {
      if (!isStale()) {
        loadError = String(err);
        notifyError(loadError);
      }
    }
  });

  let busy = $derived(submoduleReload.busy || updateBusy);

  $effect(() => {
    void submoduleReload.reload(repoPath, refreshKey);
  });

  function runAction(fn: () => Promise<void>) {
    void submoduleReload.runAction(repoPath, fn, notifyError, onChanged);
  }

  function handleInit(entry: SubmoduleInfo) {
    runAction(async () => {
      await initSubmodule(repoPath, entry.name);
      notifySuccess(`Initialized "${entry.name}".`);
    });
  }

  function handleSync(entry: SubmoduleInfo) {
    runAction(async () => {
      await syncSubmodule(repoPath, entry.name);
      notifySuccess(`Synced "${entry.name}" to .gitmodules.`);
    });
  }

  async function runUpdate(name: string | undefined, label: string) {
    if (busy) return;
    updateBusy = true;
    updateProgress = null;
    try {
      await updateSubmodule(repoPath, name, recursive, (p) => (updateProgress = p));
      notifySuccess(`Updated ${label}.`);
      await submoduleReload.reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      const message = String(err);
      if (/operation cancelled/i.test(message)) {
        notifySuccess("Cancelled.");
      } else {
        notifyError(message);
      }
    } finally {
      updateBusy = false;
      updateProgress = null;
    }
  }

  function handleUpdate(entry: SubmoduleInfo) {
    void runUpdate(entry.name, `"${entry.name}"`);
  }

  function handleUpdateAll() {
    void runUpdate(undefined, "all submodules");
  }

  function handleCancel() {
    void cancelSubmoduleUpdate(repoPath);
  }

  function progressLabel(p: RemoteProgress): string {
    return p.current != null && p.total != null
      ? `${p.phase}: ${p.percent}% (${p.current}/${p.total})`
      : `${p.phase}: ${p.percent}%`;
  }

  // The backend reports paths relative to the repo root, as git itself does.
  function absolutePath(entry: SubmoduleInfo): string {
    return `${repoPath.replace(/[/\\]+$/, "")}/${entry.path}`;
  }

  function handleOpen(entry: SubmoduleInfo) {
    onOpen(absolutePath(entry));
  }
</script>

<div class="submodule-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  {#if submodules.length > 0}
    <ul class="submodule-list">
      {#each submodules as entry (entry.name)}
        <li>
          <div class="submodule-row">
            <span class="submodule-name" title={absolutePath(entry)}>{entry.name}</span>
            {#if !entry.isInitialized}
              <span class="badge uninitialized">Not initialized</span>
            {:else if entry.isMissing}
              <span class="badge missing">Missing</span>
            {/if}
            {#if entry.needsUpdate}
              <span class="badge needs-update">Needs update</span>
            {/if}
            {#if entry.isDirty}
              <span class="badge dirty">Uncommitted changes</span>
            {/if}
          </div>
          {#if entry.path !== entry.name}
            <div class="submodule-path" title={absolutePath(entry)}>{entry.path}</div>
          {/if}
          <div class="submodule-actions">
            {#if !entry.isInitialized}
              <button type="button" onclick={() => handleInit(entry)} disabled={busy}>
                Init
              </button>
            {/if}
            <button type="button" onclick={() => handleUpdate(entry)} disabled={busy}>
              Update
            </button>
            <button type="button" onclick={() => handleSync(entry)} disabled={busy}>
              Sync
            </button>
            <button
              type="button"
              onclick={() => handleOpen(entry)}
              disabled={busy || entry.isMissing}
              title={entry.isMissing ? "Nothing valid to open — run Update first" : "Open"}
            >
              Open
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {:else if !loadError}
    <p class="empty">No submodules in this repository.</p>
  {/if}

  <div class="update-all">
    <label class="recursive-toggle">
      <input type="checkbox" bind:checked={recursive} disabled={busy} />
      Recursive (include nested submodules)
    </label>
    <div class="update-all-row">
      <button
        type="button"
        onclick={handleUpdateAll}
        disabled={busy || submodules.length === 0}
      >
        Update all
      </button>
      {#if updateBusy}
        <button type="button" onclick={handleCancel}>Cancel</button>
      {/if}
    </div>
    {#if updateBusy && updateProgress}
      <p class="progress-label" role="status">{progressLabel(updateProgress)}</p>
    {/if}
  </div>
</div>

<style>
  .submodule-panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 24rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .empty {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .submodule-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    max-height: 16rem;
    overflow-y: auto;
  }

  .submodule-list li {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.3rem 0.35rem;
    border-radius: var(--radius-sm);
  }

  .submodule-list li:hover {
    background: var(--surface-2);
  }

  .submodule-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.4rem;
    min-width: 0;
  }

  .submodule-name {
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

  .badge.dirty {
    color: var(--warning);
    border-color: var(--warning);
  }

  .badge.needs-update {
    color: var(--warning);
    border-color: var(--warning);
  }

  .badge.missing,
  .badge.uninitialized {
    color: var(--danger);
    border-color: var(--danger);
  }

  .submodule-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .submodule-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .submodule-actions button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .submodule-actions button:not(:disabled):hover {
    background: var(--surface-2);
  }

  .update-all {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .recursive-toggle {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .update-all-row {
    display: flex;
    gap: 0.35rem;
  }

  .update-all-row button {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .update-all-row button:not(:disabled):hover {
    background: var(--surface-2);
  }

  .progress-label {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-secondary);
  }
</style>
