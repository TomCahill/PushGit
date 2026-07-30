<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Stash panel: lists stashes with apply/pop/drop/rename actions, and a form to create a
  // new one from the current working directory. Stashing here always includes untracked files (see
  // `stash::create_stash`'s doc comment) and the whole working tree — selective (per-file)
  // stashing lives in `StagingPanel`'s own "Stash" button per file row instead.
  import {
    applyStash,
    createStash,
    dropStash,
    listStashes,
    popStash,
    renameStash,
  } from "$lib/git/api";
  import { confirmAsync, promptAsync } from "$lib/shell/confirmDialog.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type { StashEntry } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onApplied,
    onCountChange,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onApplied?: () => void;
    onCountChange?: (count: number) => void;
  } = $props();

  let stashes = $state<StashEntry[]>([]);
  let loadError = $state<string | null>(null);

  let busy = $state(false);

  let newStashMessage = $state("");

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      stashes = [];
      onCountChange?.(0);
      return;
    }

    loadError = null;
    try {
      const list = await listStashes(path);
      if (myGeneration !== generation) return;
      stashes = list;
      onCountChange?.(list.length);
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

  function handleCreateSubmit(event: SubmitEvent) {
    event.preventDefault();
    const message = newStashMessage.trim();
    void runAction(async () => {
      await createStash(repoPath, message || undefined);
      newStashMessage = "";
      notifySuccess("Stashed changes.");
    });
  }

  function handleApply(entry: StashEntry) {
    void runAction(async () => {
      await applyStash(repoPath, entry.index);
      notifySuccess(`Applied stash "${entry.message}".`);
      onApplied?.();
    });
  }

  function handlePop(entry: StashEntry) {
    void runAction(async () => {
      await popStash(repoPath, entry.index);
      notifySuccess(`Popped stash "${entry.message}".`);
      onApplied?.();
    });
  }

  async function handleDrop(entry: StashEntry) {
    if (!(await confirmAsync(`Drop stash "${entry.message}"? This can't be undone.`))) return;
    void runAction(async () => {
      await dropStash(repoPath, entry.index);
      notifySuccess(`Dropped stash "${entry.message}".`);
    });
  }

  async function handleRename(entry: StashEntry) {
    const newMessage = (
      await promptAsync(`Rename stash "${entry.message}" to:`, entry.message)
    )?.trim();
    if (!newMessage || newMessage === entry.message) return;
    void runAction(async () => {
      await renameStash(repoPath, entry.index, newMessage);
      notifySuccess(`Renamed stash to "${newMessage}".`);
    });
  }
</script>

<div class="stash-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  {#if stashes.length === 0}
    <p class="placeholder">No stashes.</p>
  {:else}
    <ul class="stash-list">
      {#each stashes as entry (entry.oid)}
        <li>
          <div class="stash-row">
            <span class="stash-message" title={entry.message}>{entry.message}</span>
            <span class="stash-oid">{entry.oid.slice(0, 7)}</span>
          </div>
          <div class="stash-actions">
            <button
              type="button"
              title={`Apply "${entry.message}"`}
              onclick={() => handleApply(entry)}
              disabled={busy}
            >
              Apply
            </button>
            <button
              type="button"
              title={`Pop "${entry.message}"`}
              onclick={() => handlePop(entry)}
              disabled={busy}
            >
              Pop
            </button>
            <button
              type="button"
              title={`Rename "${entry.message}"`}
              onclick={() => handleRename(entry)}
              disabled={busy}
            >
              Rename
            </button>
            <button
              type="button"
              title={`Drop "${entry.message}"`}
              onclick={() => handleDrop(entry)}
              disabled={busy}
            >
              Drop
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <form class="create-stash" onsubmit={handleCreateSubmit}>
    <label for="new-stash-message">Stash message (optional)</label>
    <div class="create-stash-row">
      <input id="new-stash-message" type="text" bind:value={newStashMessage} placeholder="WIP" />
      <button type="submit" disabled={busy}>Stash changes</button>
    </div>
  </form>
</div>

<style>
  .stash-panel {
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

  .stash-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .stash-list li {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.25rem 0.3rem;
    border-radius: var(--radius-sm);
  }

  .stash-list li:hover {
    background: var(--surface-2);
  }

  .stash-row {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
    min-width: 0;
  }

  .stash-message {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .stash-oid {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .stash-actions {
    display: flex;
    gap: 0.15rem;
    flex-shrink: 0;
  }

  .stash-actions button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .stash-actions button:hover {
    background: var(--surface-2);
  }

  .create-stash {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .create-stash label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .create-stash-row {
    display: flex;
    gap: 0.35rem;
  }

  .create-stash-row input {
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

  .create-stash-row input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .create-stash-row button {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .create-stash-row button:not(:disabled):hover {
    background: var(--surface-2);
  }
</style>
