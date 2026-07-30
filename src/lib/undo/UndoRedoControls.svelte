<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Shared Undo/Redo toolbar controls, backed by the app-level undo/redo stack.
  // Replaces the three separate
  // "Undo last operation" single-slot buttons that used to live independently in
  // BranchSidebar/RemotePanel/CommitGraph (`snapshotHead`/`undoToSnapshot`) — those actions,
  // plus commit/checkout/stash/tag-delete/conflict-resolution/discard, now all record into
  // one backend stack per repo, so one pair of buttons here covers every wrapped action
  // regardless of which panel triggered it.
  import { redoLastOperation, undoLastOperation, undoRedoStatus } from "$lib/git/api";
  import Icon from "$lib/shell/Icon.svelte";

  let {
    repoPath,
    refreshKey,
    onChanged,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
  } = $props();

  let canUndo = $state(false);
  let undoLabel = $state<string | null>(null);
  let canRedo = $state(false);
  let redoLabel = $state<string | null>(null);

  let busy = $state(false);
  let actionError = $state<string | null>(null);

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      canUndo = false;
      undoLabel = null;
      canRedo = false;
      redoLabel = null;
      return;
    }

    try {
      const status = await undoRedoStatus(path);
      if (myGeneration !== generation) return;
      canUndo = status.canUndo;
      undoLabel = status.undoLabel;
      canRedo = status.canRedo;
      redoLabel = status.redoLabel;
    } catch {
      // Best-effort: an undo/redo status hiccup shouldn't block the rest of the toolbar.
    }
  }

  async function runAction(fn: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    actionError = null;
    try {
      await fn();
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      actionError = String(err);
    } finally {
      busy = false;
    }
  }

  function handleUndo() {
    void runAction(() => undoLastOperation(repoPath));
  }

  function handleRedo() {
    void runAction(() => redoLastOperation(repoPath));
  }
</script>

<div class="undo-redo">
  <button
    type="button"
    onclick={handleUndo}
    disabled={busy || !canUndo}
    title={canUndo ? `Undo: ${undoLabel}` : "Nothing to undo"}
    aria-label="Undo last operation"
  >
    <Icon name="undo" size={13} />
  </button>
  <button
    type="button"
    onclick={handleRedo}
    disabled={busy || !canRedo}
    title={canRedo ? `Redo: ${redoLabel}` : "Nothing to redo"}
    aria-label="Redo last undone operation"
  >
    <Icon name="redo" size={13} />
  </button>
  {#if actionError}
    <p class="error" role="alert">{actionError}</p>
  {/if}
</div>

<style>
  .undo-redo {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0.35rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  button:not(:disabled):hover {
    background: var(--surface-2);
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 0.72rem;
  }
</style>
