<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Blame + file history — secondary investigative tools, not
  // part of the core commit/diff/merge loop. Blame is computed against HEAD only (not an
  // arbitrary historical revision) and shown as plain text, not syntax-highlighted —
  // `HunkDiff.svelte`'s per-hunk highlighting machinery is built around diff hunks, not a
  // whole-file line list, and duplicating it here is more than this view needs. This
  // component owns only the sidebar's file-history list; the annotated blame lines
  // (`BlameFileView.svelte`) and a selected history entry's diff (`HunkDiff`) both render
  // in the shell's center panel instead, reported up via `onLinesChange`/`onDiffChange` —
  // the same "sidebar picks, center shows" split `StagingPanel` uses.
  import { blameFile, diffCommit, fileHistory } from "$lib/git/api";
  import type { BlameLine, FileDiff, FileDiffSelection, FileHistoryEntry } from "$lib/git/types";
  import Avatar from "$lib/shell/Avatar.svelte";

  let {
    repoPath,
    path,
    onClose,
    onDiffChange,
    onLinesChange,
  }: {
    repoPath: string;
    path: string;
    onClose: () => void;
    /** Fires whenever the selected history entry's diff changes, so the shell can render
     *  `HunkDiff` for it in the center panel — this view no longer renders the diff itself. */
    onDiffChange?: (diff: FileDiffSelection | null) => void;
    /** Fires whenever the blamed file's annotated lines (re)load, so the shell can render
     *  `BlameFileView` for them in the center panel in place of the commit graph. */
    onLinesChange?: (lines: BlameLine[] | null) => void;
  } = $props();

  let lines = $state<BlameLine[] | null>(null);
  let history = $state<FileHistoryEntry[] | null>(null);
  let loadError = $state<string | null>(null);

  let selectedHistoryOid = $state<string | null>(null);
  let historyDiff = $state<FileDiff | null>(null);
  let historyDiffError = $state<string | null>(null);

  $effect(() => {
    void load(repoPath, path);
  });

  async function load(rp: string, p: string) {
    lines = null;
    history = null;
    loadError = null;
    selectedHistoryOid = null;
    historyDiff = null;
    try {
      const [blameResult, historyResult] = await Promise.all([
        blameFile(rp, p),
        fileHistory(rp, p),
      ]);
      lines = blameResult;
      history = historyResult;
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleSelectHistory(entry: FileHistoryEntry) {
    selectedHistoryOid = entry.oid;
    historyDiff = null;
    historyDiffError = null;
    try {
      const files = await diffCommit(repoPath, entry.oid);
      historyDiff = files.find((f) => f.newPath === path || f.oldPath === path) ?? null;
    } catch (err) {
      historyDiffError = String(err);
    }
  }

  $effect(() => {
    onDiffChange?.(historyDiff ? { file: historyDiff } : null);
  });

  $effect(() => {
    onLinesChange?.(lines);
  });

  function formatDate(unixSeconds: number): string {
    return new Date(unixSeconds * 1000).toLocaleDateString();
  }
</script>

<div class="blame-view">
  <div class="header">
    <h2>{path}</h2>
    <button type="button" onclick={onClose}>Close</button>
  </div>

  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {:else if lines === null || history === null}
    <p class="loading">Loading…</p>
  {:else}
    <section class="history" aria-label="File history">
      <h3>History ({history.length})</h3>
      {#if history.length === 0}
        <p class="placeholder">No history found for this file.</p>
      {:else}
        <ul>
          {#each history as entry (entry.oid)}
            <li>
              <button
                type="button"
                class="history-entry"
                class:active={selectedHistoryOid === entry.oid}
                onclick={() => handleSelectHistory(entry)}
              >
                <span class="short-oid">{entry.shortOid}</span>
                <span class="summary">{entry.summary}</span>
                <span class="meta"
                  ><Avatar name={entry.authorName} size={14} /> - {formatDate(
                    entry.authorTime,
                  )}</span
                >
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if selectedHistoryOid && historyDiffError}
        <p class="error" role="alert">{historyDiffError}</p>
      {/if}
    </section>
  {/if}
</div>

<style>
  .blame-view {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    height: 100%;
    min-height: 0;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .header h2 {
    margin: 0;
    font-size: 0.95rem;
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header button {
    flex-shrink: 0;
    padding: 0.3rem 0.7rem;
    font: inherit;
    font-size: 0.8rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .loading,
  .placeholder {
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .history {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    min-height: 0;
    overflow-y: auto;
  }

  .history h3 {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .history ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .history-entry {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    width: 100%;
    padding: 0.35rem 0.5rem;
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .history-entry:hover {
    background: var(--surface-2);
  }

  .history-entry.active {
    background: var(--accent-bg);
  }

  .history-entry .summary {
    font-size: 0.82rem;
    color: var(--text-primary);
  }

  .history-entry .meta {
    font-size: 0.7rem;
    color: var(--text-muted);
  }
</style>
