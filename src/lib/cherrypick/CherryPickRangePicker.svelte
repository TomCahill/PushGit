<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Multi-commit cherry-pick: search for commits, check off a
  // range, and apply them onto HEAD in chronological order via real `git cherry-pick
  // <oid>...` (`src-tauri/src/cherry_pick_range/mod.rs` — git2 has no sequencer/queue API
  // at all, only single-commit cherry-pick). Conflict recovery here is its own
  // continue/abort pair (`continueCherryPickRange`/`abortCherryPickRange`), never the
  // ordinary commit box `cherryPickCommit`'s conflicts use — see that Rust module's doc
  // comment for why `git cherry-pick --continue` (which both commits *and* advances the
  // sequence) can't be mixed with the single-commit path's plain "stage then hit Commit".
  import { cherryPickRange, continueCherryPickRange, abortCherryPickRange } from "$lib/git/api";
  import { graphOpen, graphPage, graphClose } from "$lib/git/api";
  import type { CherryPickOutcome, CommitRow } from "$lib/git/types";

  const SEARCH_DEBOUNCE_MS = 200;
  const MAX_RESULTS = 20;

  let {
    repoPath,
    onCompleted,
    onConflicts,
    onCancel,
  }: {
    repoPath: string;
    onCompleted: () => void;
    onConflicts: () => void;
    onCancel: () => void;
  } = $props();

  let query = $state("");
  let results = $state<CommitRow[]>([]);
  let selected = $state<Map<string, CommitRow>>(new Map());

  let busy = $state(false);
  let error = $state<string | null>(null);
  let conflicts = $state<string[]>([]);
  let started = $state(false);

  const selectedCount = $derived(selected.size);

  let searchGeneration = 0;
  $effect(() => {
    const q = query;
    const myGeneration = ++searchGeneration;
    if (!q.trim()) {
      results = [];
      return;
    }
    const timer = setTimeout(() => void search(q, myGeneration), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  async function search(q: string, generation: number) {
    let sessionId: string | null = null;
    try {
      sessionId = await graphOpen(repoPath, { refs: [], search: q });
      const page = await graphPage(sessionId, MAX_RESULTS);
      if (generation === searchGeneration) results = page.rows;
    } catch (err) {
      if (generation === searchGeneration) error = String(err);
    } finally {
      if (sessionId) void graphClose(sessionId);
    }
  }

  function toggle(commit: CommitRow) {
    const next = new Map(selected);
    if (next.has(commit.oid)) {
      next.delete(commit.oid);
    } else {
      next.set(commit.oid, commit);
    }
    selected = next;
  }

  function describeOutcome(outcome: CherryPickOutcome): { conflicted: boolean } {
    if (outcome.kind === "conflicts") {
      conflicts = outcome.conflicts;
      return { conflicted: true };
    }
    conflicts = [];
    return { conflicted: false };
  }

  async function handleStart() {
    if (busy || selected.size === 0) return;
    busy = true;
    error = null;
    try {
      // Chronological (oldest-first) order — the only sensible default for applying a
      // range picked from search results in no particular order.
      const oids = [...selected.values()]
        .sort((a, b) => a.authorTime - b.authorTime)
        .map((c) => c.oid);
      started = true;
      const outcome = await cherryPickRange(repoPath, oids);
      const { conflicted } = describeOutcome(outcome);
      if (conflicted) {
        onConflicts();
      } else {
        onCompleted();
      }
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function handleContinue() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const outcome = await continueCherryPickRange(repoPath);
      const { conflicted } = describeOutcome(outcome);
      if (conflicted) {
        onConflicts();
      } else {
        onCompleted();
      }
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function handleAbort() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await abortCherryPickRange(repoPath);
      onCancel();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="picker">
  <h2>Cherry-pick commits</h2>

  {#if conflicts.length > 0}
    <div class="conflict-banner" role="alert">
      <p>
        Cherry-pick paused — {conflicts.length} conflicting file{conflicts.length === 1 ? "" : "s"}.
        Resolve them in the working directory view, then continue.
      </p>
      <div class="conflict-actions">
        <button type="button" onclick={handleContinue} disabled={busy}>Continue cherry-pick</button>
        <button type="button" onclick={handleAbort} disabled={busy}>Abort cherry-pick</button>
      </div>
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if !started || conflicts.length > 0}
    {#if selectedCount > 0}
      <ul class="selected-list" aria-label="Selected commits">
        {#each [...selected.values()].sort((a, b) => a.authorTime - b.authorTime) as commit (commit.oid)}
          <li>
            <span class="short-oid">{commit.shortOid}</span>
            <span class="summary">{commit.summary}</span>
            <button
              type="button"
              class="remove"
              title="Remove from selection"
              disabled={started}
              onclick={() => toggle(commit)}
            >
              ⨯
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <input
      type="text"
      class="search-box"
      bind:value={query}
      placeholder="Search commits by message, author, or SHA…"
      aria-label="Search commits to cherry-pick"
      disabled={started}
    />

    {#if results.length > 0}
      <ul class="results" role="listbox" aria-label="Search results">
        {#each results as commit (commit.oid)}
          <li>
            <label>
              <input
                type="checkbox"
                checked={selected.has(commit.oid)}
                disabled={started}
                onchange={() => toggle(commit)}
              />
              <span class="short-oid">{commit.shortOid}</span>
              <span class="summary">{commit.summary}</span>
            </label>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}

  {#if !started}
    <div class="footer-actions">
      <button type="button" onclick={onCancel} disabled={busy}>Cancel</button>
      <button
        type="button"
        class="primary"
        onclick={handleStart}
        disabled={busy || selectedCount === 0}
      >
        Cherry-pick {selectedCount} commit{selectedCount === 1 ? "" : "s"}
      </button>
    </div>
  {/if}
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  .error {
    margin: 0;
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

  .search-box {
    padding: 0.4rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .selected-list,
  .results {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    max-height: 14rem;
    overflow-y: auto;
  }

  .selected-list li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.5rem;
    background: var(--accent-bg);
    border-radius: var(--radius-sm);
  }

  .results li label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem 0.5rem;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .results li label:hover {
    background: var(--surface-2);
  }

  .short-oid {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .summary {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
  }

  .remove {
    flex-shrink: 0;
    width: 1.4rem;
    height: 1.4rem;
    color: var(--text-secondary);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .remove:hover {
    background: var(--danger-bg);
    color: var(--danger);
  }

  .footer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  .footer-actions button {
    padding: 0.4rem 0.9rem;
    font: inherit;
    font-size: 0.85rem;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--surface-1);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .footer-actions button.primary {
    color: var(--btn-filled-fg);
    background: var(--btn-filled-bg);
    border-color: transparent;
  }

  .footer-actions button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
