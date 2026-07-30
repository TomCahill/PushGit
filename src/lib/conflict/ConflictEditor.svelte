<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // 3-way merge-conflict editor: shows the
  // base/ours/theirs sides of one conflicted path and lets the user resolve it per-hunk
  // (take ours / take theirs / edit manually) with a live preview of the resolved file
  // before it's marked resolved, wired to `conflict_sides`/`write_resolved_conflict`/
  // `resolve_conflict_as_deleted`. The *ours-vs-theirs* diff — not a base-relative 3-way
  // merge — drives per-hunk resolution, since that's the actual disagreement needing a
  // decision; base is shown read-only, for reference/context only.
  import type { ConflictSides, Hunk, Line } from "$lib/git/types";
  import { conflictSides, resolveConflictAsDeleted, writeResolvedConflict } from "$lib/git/api";
  import { highlightSource, splitHighlightedHtml } from "$lib/diff/highlight";
  import { detectLanguage } from "$lib/diff/languages";
  import { pairHunkLines } from "$lib/diff/pairHunkLines";
  import { buildResolvedContent, type HunkDecision } from "./buildResolvedContent";

  let {
    repoPath,
    path,
    onResolved,
    onCancel,
  }: {
    repoPath: string;
    path: string;
    onResolved: () => void;
    onCancel: () => void;
  } = $props();

  let sides = $state<ConflictSides | null>(null);
  let loadError = $state<string | null>(null);
  let actionError = $state<string | null>(null);
  let submitting = $state(false);
  let decisions = $state<Map<number, HunkDecision>>(new Map());
  let generation = 0;

  $effect(() => {
    void load(repoPath, path);
  });

  async function load(rp: string, p: string) {
    const myGeneration = ++generation;
    sides = null;
    loadError = null;
    decisions = new Map();
    try {
      const result = await conflictSides(rp, p);
      if (myGeneration === generation) sides = result;
    } catch (err) {
      if (myGeneration === generation) loadError = String(err);
    }
  }

  const language = $derived(detectLanguage(path));

  function sideText(hunk: Hunk, side: "ours" | "theirs"): string {
    const wanted = side === "ours" ? ["context", "deletion"] : ["context", "addition"];
    return hunk.lines
      .filter((line) => wanted.includes(line.origin))
      .map((line) => line.content)
      .join("");
  }

  const highlightedByHunk = $derived.by(() => {
    const map = new Map<number, string[]>();
    if (!sides || !language) return map;
    sides.hunks.forEach((hunk, index) => {
      const source = hunk.lines.map((line) => line.content).join("");
      const html = highlightSource(source, language);
      if (html !== null) map.set(index, splitHighlightedHtml(html, hunk.lines.length));
    });
    return map;
  });

  function lineText(line: Line): string {
    return line.content.endsWith("\n") ? line.content.slice(0, -1) : line.content;
  }

  function lineHtml(hunkIndex: number, lineIndex: number): string | null {
    return highlightedByHunk.get(hunkIndex)?.[lineIndex] ?? null;
  }

  function setDecision(index: number, decision: HunkDecision) {
    const next = new Map(decisions);
    next.set(index, decision);
    decisions = next;
  }

  function editManually(index: number, hunk: Hunk) {
    const current = decisions.get(index);
    const seed = current?.kind === "manual" ? current.content : sideText(hunk, "theirs");
    setDecision(index, { kind: "manual", content: seed });
  }

  function updateManualContent(
    index: number,
    event: Event & { currentTarget: HTMLTextAreaElement },
  ) {
    setDecision(index, { kind: "manual", content: event.currentTarget.value });
  }

  const resolvedContent = $derived.by(() => {
    if (!sides || sides.ours === null) return null;
    return buildResolvedContent(sides.ours, sides.hunks, decisions);
  });

  const baseHtml = $derived.by(() =>
    sides?.base && language ? highlightSource(sides.base, language) : null,
  );

  const resolvedHtml = $derived.by(() =>
    resolvedContent !== null && language ? highlightSource(resolvedContent, language) : null,
  );

  const allDecided = $derived(sides !== null && decisions.size === sides.hunks.length);
  const keepableContent = $derived(sides?.ours ?? sides?.theirs ?? null);

  async function run(action: () => Promise<void>) {
    if (submitting) return;
    submitting = true;
    actionError = null;
    try {
      await action();
      onResolved();
    } catch (err) {
      actionError = String(err);
    } finally {
      submitting = false;
    }
  }

  function markResolved() {
    if (resolvedContent === null) return;
    const content = resolvedContent;
    void run(() => writeResolvedConflict(repoPath, path, content));
  }

  function keepWhole(content: string) {
    void run(() => writeResolvedConflict(repoPath, path, content));
  }

  function keepDeleted() {
    void run(() => resolveConflictAsDeleted(repoPath, path));
  }
</script>

<div class="conflict-editor">
  <div class="header">
    <h3>{path}</h3>
    <button type="button" class="cancel-button" onclick={onCancel}>Cancel</button>
  </div>

  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {:else if !sides}
    <p class="placeholder">Loading…</p>
  {:else if sides.isBinary}
    <p class="placeholder">Binary file — pick a side to keep, whole file.</p>
    <div class="whole-file-actions">
      <button
        type="button"
        disabled={sides.ours === null}
        onclick={() => sides!.ours !== null && keepWhole(sides!.ours)}
      >
        Keep ours
      </button>
      <button
        type="button"
        disabled={sides.theirs === null}
        onclick={() => sides!.theirs !== null && keepWhole(sides!.theirs)}
      >
        Keep theirs
      </button>
    </div>
  {:else if sides.ours === null || sides.theirs === null}
    <p class="placeholder">
      This file was deleted on {sides.ours === null ? "our" : "their"} side and
      {sides.ours === null ? "modified" : "deleted"} on the other.
    </p>
    <div class="whole-file-actions">
      <button
        type="button"
        disabled={keepableContent === null}
        onclick={() => keepableContent !== null && keepWhole(keepableContent)}
      >
        Keep the file
      </button>
      <button type="button" onclick={keepDeleted}>Delete the file</button>
    </div>
  {:else}
    {#if sides.base !== null}
      <details class="reference">
        <summary>Base (common ancestor)</summary>
        <pre class="pane"><code
            >{#if baseHtml}{@html baseHtml}{:else}{sides.base}{/if}</code
          ></pre>
      </details>
    {/if}

    <div class="hunks">
      {#each sides.hunks as hunk, index (index)}
        {@const decision = decisions.get(index)}
        <div class="hunk">
          <div class="hunk-header">
            <span class="hunk-label">{hunk.header}</span>
            <div class="hunk-actions">
              <button
                type="button"
                class:active={decision?.kind === "ours"}
                onclick={() => setDecision(index, { kind: "ours" })}
              >
                Take ours
              </button>
              <button
                type="button"
                class:active={decision?.kind === "theirs"}
                onclick={() => setDecision(index, { kind: "theirs" })}
              >
                Take theirs
              </button>
              <button
                type="button"
                class:active={decision?.kind === "manual"}
                onclick={() => editManually(index, hunk)}
              >
                Edit manually
              </button>
            </div>
          </div>

          {#if decision?.kind === "manual"}
            <textarea
              class="manual-edit"
              value={decision.content}
              oninput={(event) => updateManualContent(index, event)}></textarea>
          {:else}
            {#each pairHunkLines(hunk.lines) as row}
              <div class="split-row">
                <div
                  class="split-cell"
                  class:empty={!row.left}
                  class:deletion={row.left?.line.origin === "deletion"}
                >
                  {#if row.left}
                    {@const html = lineHtml(index, row.left.index)}
                    {#if html}<span class="content">{@html html}</span>{:else}<span class="content"
                        >{lineText(row.left.line)}</span
                      >{/if}
                  {/if}
                </div>
                <div
                  class="split-cell"
                  class:empty={!row.right}
                  class:addition={row.right?.line.origin === "addition"}
                >
                  {#if row.right}
                    {@const html = lineHtml(index, row.right.index)}
                    {#if html}<span class="content">{@html html}</span>{:else}<span class="content"
                        >{lineText(row.right.line)}</span
                      >{/if}
                  {/if}
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {/each}
    </div>

    <details class="reference" open>
      <summary>Resolved preview</summary>
      <pre class="pane"><code
          >{#if resolvedHtml}{@html resolvedHtml}{:else}{resolvedContent}{/if}</code
        ></pre>
    </details>

    {#if actionError}
      <p class="error" role="alert">{actionError}</p>
    {/if}

    <div class="footer-actions">
      <button type="button" disabled={!allDecided || submitting} onclick={markResolved}>
        Mark resolved
      </button>
    </div>
  {/if}
</div>

<style>
  .conflict-editor {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .header h3 {
    margin: 0;
    font-size: 0.9rem;
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  .cancel-button {
    flex-shrink: 0;
    padding: 0.3rem 0.6rem;
    font-size: 0.75rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .cancel-button:hover {
    background: var(--surface-2);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .placeholder {
    color: var(--text-muted);
  }

  .whole-file-actions,
  .footer-actions {
    display: flex;
    gap: 0.5rem;
  }

  .whole-file-actions button,
  .footer-actions button {
    padding: 0.4rem 0.8rem;
    font-size: 0.82rem;
    color: var(--text-primary);
    background: var(--accent-bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .whole-file-actions button:disabled,
  .footer-actions button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .reference {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .reference summary {
    padding: 0.4rem 0.6rem;
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--surface-2);
    cursor: pointer;
  }

  .pane {
    margin: 0;
    padding: 0.6rem;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    white-space: pre-wrap;
    overflow-x: auto;
  }

  .hunks {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .hunk {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .hunk-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.3rem 0.6rem;
    background: var(--surface-2);
  }

  .hunk-label {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .hunk-actions {
    display: flex;
    gap: 0.3rem;
  }

  .hunk-actions button {
    padding: 0.2rem 0.5rem;
    font-size: 0.72rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .hunk-actions button:hover {
    background: var(--surface-0);
  }

  .hunk-actions button.active {
    color: var(--accent);
    background: var(--accent-bg);
    border-color: var(--accent);
  }

  .split-row {
    display: flex;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    white-space: pre;
  }

  .split-cell {
    flex: 1 1 50%;
    min-width: 0;
    padding: 0 0.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .split-cell:first-child {
    border-right: 1px solid var(--border);
  }

  .split-cell.deletion {
    background: var(--danger-bg);
  }

  .split-cell.addition {
    background: var(--success-bg);
  }

  .split-cell.empty {
    background: var(--surface-2);
  }

  .manual-edit {
    box-sizing: border-box;
    width: 100%;
    min-height: 6rem;
    padding: 0.5rem;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: none;
    border-top: 1px solid var(--border);
    resize: vertical;
  }
</style>
