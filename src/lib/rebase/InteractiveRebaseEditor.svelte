<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Interactive rebase editor: a reorderable list of the commits
  // between `onto` and HEAD, each assignable to pick/squash/reword/drop, then replayed via
  // `start_interactive_rebase` — a thin UI over real `git rebase -i` (`src-tauri/src/
  // interactive_rebase/mod.rs`; libgit2 has no interactive-rebase support at all).
  // Conflict recovery here is deliberately its own continue/abort pair
  // (`continue_interactive_rebase`/`abort_interactive_rebase`), never `BranchSidebar`'s
  // plain-rebase `continueRebase`/`abortRebase` — see that Rust module's doc comment for why
  // mixing the two recovery paths would be a real (not just cosmetic) risk.
  import { untrack } from "svelte";
  import {
    abortInteractiveRebase,
    continueInteractiveRebase,
    startInteractiveRebase,
  } from "$lib/git/api";
  import Button from "$lib/shell/Button.svelte";
  import type {
    RebaseAction,
    RebaseCommitSummary,
    RebaseOutcome,
    RebaseStep,
  } from "$lib/git/types";

  let {
    repoPath,
    onto,
    commits,
    onCompleted,
    onConflicts,
    onCancel,
  }: {
    repoPath: string;
    onto: string;
    commits: RebaseCommitSummary[];
    onCompleted: () => void;
    onConflicts: () => void;
    onCancel: () => void;
  } = $props();

  type Row = RebaseCommitSummary & { action: RebaseAction; rewordDraft: string };

  // `commits` only ever seeds this component's own reorderable state — the parent mounts a
  // fresh instance (`{#key rebaseOnto}`) for each new rebase rather than mutating `commits`
  // on an existing one, so intentionally not reacting to later changes here.
  let rows = $state<Row[]>(
    untrack(() => commits.map((c) => ({ ...c, action: { kind: "pick" }, rewordDraft: c.summary }))),
  );
  let dragIndex = $state<number | null>(null);
  let dropIndex = $state<number | null>(null);

  let started = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let conflicts = $state<string[]>([]);

  function describeOutcome(outcome: RebaseOutcome): { conflicted: boolean } {
    if (outcome.kind === "conflicts") {
      conflicts = outcome.conflicts;
      return { conflicted: true };
    }
    conflicts = [];
    return { conflicted: false };
  }

  function toSteps(): RebaseStep[] {
    return rows.map((row) => ({
      oid: row.oid,
      action:
        row.action.kind === "reword" ? { kind: "reword", message: row.rewordDraft } : row.action,
    }));
  }

  async function handleStart() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      started = true;
      const outcome = await startInteractiveRebase(repoPath, onto, toSteps());
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
      const outcome = await continueInteractiveRebase(repoPath);
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
      await abortInteractiveRebase(repoPath);
      onCancel();
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  function setAction(index: number, kind: RebaseAction["kind"]) {
    const row = rows[index];
    if (!row) return;
    row.action = kind === "reword" ? { kind: "reword", message: row.rewordDraft } : { kind };
  }

  function handleDragStart(index: number) {
    dragIndex = index;
  }

  function handleDragOver(event: DragEvent, index: number) {
    event.preventDefault();
    dropIndex = index;
  }

  function handleDrop(event: DragEvent, index: number) {
    event.preventDefault();
    const from = dragIndex;
    dragIndex = null;
    dropIndex = null;
    if (from === null || from === index) return;
    const next = rows.slice();
    const [moved] = next.splice(from, 1);
    next.splice(index, 0, moved);
    rows = next;
  }

  function handleDragEnd() {
    dragIndex = null;
    dropIndex = null;
  }
</script>

<div class="editor">
  <h2>Interactive rebase onto "{onto}"</h2>

  {#if conflicts.length > 0}
    <div class="conflict-banner" role="alert">
      <p>
        Rebase paused — {conflicts.length} conflicting file{conflicts.length === 1 ? "" : "s"}.
        Resolve them in the working directory view, then continue.
      </p>
      <div class="conflict-actions">
        <button type="button" onclick={handleContinue} disabled={busy}>Continue rebase</button>
        <button type="button" onclick={handleAbort} disabled={busy}>Abort rebase</button>
      </div>
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if !started || conflicts.length > 0}
    <ul class="rows" class:disabled={started}>
      {#each rows as row, i (row.oid)}
        <li
          class="row"
          class:drop-target={dropIndex === i && dragIndex !== null && dragIndex !== i}
          draggable={!started}
          ondragstart={() => handleDragStart(i)}
          ondragover={(event) => handleDragOver(event, i)}
          ondrop={(event) => handleDrop(event, i)}
          ondragend={handleDragEnd}
        >
          <span class="drag-handle" aria-hidden="true">⠿</span>
          <span class="short-oid">{row.shortOid}</span>
          {#if row.action.kind === "reword"}
            <textarea
              class="reword-input"
              bind:value={row.rewordDraft}
              disabled={started}
              aria-label={`New message for ${row.shortOid}`}
              oninput={() => (row.action = { kind: "reword", message: row.rewordDraft })}
            ></textarea>
          {:else}
            <span class="summary" class:dropped={row.action.kind === "drop"}>{row.summary}</span>
          {/if}
          <div class="action-buttons" role="group" aria-label={`Action for ${row.shortOid}`}>
            <button
              type="button"
              class:active={row.action.kind === "pick"}
              disabled={started}
              onclick={() => setAction(i, "pick")}
            >
              Pick
            </button>
            <button
              type="button"
              class:active={row.action.kind === "squash"}
              disabled={started || i === 0}
              title={i === 0 ? "Can't squash the first commit — nothing to squash into" : "Squash"}
              onclick={() => setAction(i, "squash")}
            >
              Squash
            </button>
            <button
              type="button"
              class:active={row.action.kind === "reword"}
              disabled={started}
              onclick={() => setAction(i, "reword")}
            >
              Reword
            </button>
            <button
              type="button"
              class:active={row.action.kind === "drop"}
              disabled={started}
              onclick={() => setAction(i, "drop")}
            >
              Drop
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  {#if !started}
    <div class="footer-actions">
      <Button variant="outlined" onclick={onCancel} disabled={busy}>Cancel</Button>
      <Button variant="filled" onclick={handleStart} disabled={busy || rows.length === 0}>
        Start Rebase
      </Button>
    </div>
  {/if}
</div>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
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

  .rows {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rows.disabled .row {
    opacity: 0.7;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.5rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .row.drop-target {
    border-color: var(--accent);
    background: var(--accent-bg);
  }

  .drag-handle {
    flex-shrink: 0;
    color: var(--text-muted);
    cursor: grab;
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

  .summary.dropped {
    text-decoration: line-through;
    color: var(--text-muted);
  }

  .reword-input {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 2.2rem;
    font: inherit;
    font-size: 0.85rem;
    padding: 0.25rem 0.4rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    resize: vertical;
  }

  .action-buttons {
    flex-shrink: 0;
    display: flex;
    gap: 0.15rem;
  }

  .action-buttons button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .action-buttons button.active {
    color: var(--accent);
    background: var(--accent-bg);
    border-color: var(--accent);
  }

  .action-buttons button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .footer-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }
</style>
