<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Tags panel: lists tags alongside branches in the sidebar and lets you create or delete
  // one. An optional target commit-ish (blank =
  // HEAD, matching `BranchSidebar`'s "create branch" default) and an optional message (blank
  // = lightweight tag, present = annotated) both pass straight through to the backend's
  // `create_tag`, which already supported both — this panel just didn't expose them.
  // Rendering a tag inline in the graph at its target commit needed no `CommitGraph`
  // changes: the graph already draws every ref in `CommitRow.refs` generically regardless
  // of kind.
  import { createTag, deleteTag, listTags } from "$lib/git/api";
  import { confirmAsync } from "$lib/shell/confirmDialog.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onCountChange,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onCountChange?: (count: number) => void;
  } = $props();

  let tags = $state<string[]>([]);
  let loadError = $state<string | null>(null);

  let busy = $state(false);

  let newTagName = $state("");
  let newTagTarget = $state("");
  let newTagMessage = $state("");

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      tags = [];
      onCountChange?.(0);
      return;
    }

    loadError = null;
    try {
      const list = await listTags(path);
      if (myGeneration !== generation) return;
      tags = list;
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
    const name = newTagName.trim();
    if (!name) return;
    const at = newTagTarget.trim() || undefined;
    const message = newTagMessage.trim() || undefined;
    void runAction(async () => {
      await createTag(repoPath, name, at, message);
      newTagName = "";
      newTagTarget = "";
      newTagMessage = "";
      notifySuccess(`Created tag "${name}".`);
    });
  }

  async function handleDelete(name: string) {
    if (!(await confirmAsync(`Delete tag "${name}"?`))) return;
    void runAction(async () => {
      await deleteTag(repoPath, name);
      notifySuccess(`Deleted tag "${name}".`);
    });
  }
</script>

<div class="tags-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  {#if tags.length === 0}
    <p class="placeholder">No tags.</p>
  {:else}
    <ul class="tag-list">
      {#each tags as name (name)}
        <li>
          <span class="tag-name">{name}</span>
          <button
            type="button"
            title={`Delete ${name}`}
            onclick={() => handleDelete(name)}
            disabled={busy}
          >
            Delete
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <form class="create-tag" onsubmit={handleCreateSubmit}>
    <label for="new-tag-name">New tag</label>
    <div class="create-tag-row">
      <input id="new-tag-name" type="text" bind:value={newTagName} placeholder="tag name" />
      <button type="submit" disabled={busy || !newTagName.trim()}>Create</button>
    </div>
    <input
      type="text"
      bind:value={newTagTarget}
      placeholder="Target commit (blank for HEAD)"
      aria-label="Target commit"
    />
    <input
      type="text"
      bind:value={newTagMessage}
      placeholder="Message (blank for a lightweight tag)"
      aria-label="Tag message"
    />
  </form>
</div>

<style>
  .tags-panel {
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

  .tag-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .tag-list li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.15rem 0.2rem;
    border-radius: var(--radius-sm);
  }

  .tag-list li:hover {
    background: var(--surface-2);
  }

  .tag-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .tag-list button {
    flex-shrink: 0;
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .tag-list button:hover {
    background: var(--surface-2);
  }

  .create-tag {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .create-tag label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .create-tag-row {
    display: flex;
    gap: 0.35rem;
  }

  .create-tag-row input,
  .create-tag > input {
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

  .create-tag-row input:focus-visible,
  .create-tag > input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .create-tag-row button {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .create-tag-row button:not(:disabled):hover {
    background: var(--surface-2);
  }
</style>
