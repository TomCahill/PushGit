<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Repo maintenance helper: shows a rough health snapshot
  // (loose object count/size, pack count/size, from `git count-objects -v`) and a one-click
  // `git gc`, both shelling out to real git — neither has a git2/libgit2 API at all.
  import { repoHealth, runGc } from "$lib/git/api";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type { RepoHealth } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
  }: {
    repoPath: string;
    refreshKey: number;
  } = $props();

  let health = $state<RepoHealth | null>(null);
  let loadError = $state<string | null>(null);
  let busy = $state(false);

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      health = null;
      return;
    }
    loadError = null;
    try {
      const result = await repoHealth(path);
      if (myGeneration === generation) health = result;
    } catch (err) {
      if (myGeneration === generation) {
        loadError = String(err);
        notifyError(loadError);
      }
    }
  }

  async function handleRunGc() {
    if (busy) return;
    busy = true;
    try {
      await runGc(repoPath);
      notifySuccess("Ran git gc.");
      await reload(repoPath, refreshKey);
    } catch (err) {
      notifyError(String(err));
    } finally {
      busy = false;
    }
  }
</script>

<div class="maintenance-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {:else if !health}
    <p class="loading">Loading…</p>
  {:else}
    <dl class="health">
      <dt>Loose objects</dt>
      <dd>{health.looseObjectCount} ({health.looseObjectSizeKib} KiB)</dd>
      <dt>Packs</dt>
      <dd>{health.packCount} ({health.packedSizeKib} KiB)</dd>
    </dl>
  {/if}

  <button type="button" onclick={handleRunGc} disabled={busy}>Run git gc</button>
</div>

<style>
  .maintenance-panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 14rem;
  }

  .loading {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 0.8rem;
  }

  .health {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.2rem 0.6rem;
    margin: 0;
    font-size: 0.82rem;
  }

  .health dt {
    color: var(--text-muted);
  }

  .health dd {
    margin: 0;
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  button {
    align-self: flex-start;
    padding: 0.3rem 0.7rem;
    font: inherit;
    font-size: 0.8rem;
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
    opacity: 0.5;
    cursor: default;
  }
</style>
