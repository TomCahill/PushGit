<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders one file's hunks, in either of two modes shared
  // app-wide via `diffViewState` (`./diffViewMode.svelte.ts`): the default unified/inline
  // form, or a side-by-side old-vs-new split (`./pairHunkLines.ts` does the alignment).
  // Syntax highlighting (`./highlight.ts` + `./languages.ts`) applies in both modes, keyed
  // off the file's extension; a file with no recognized extension falls back to plain
  // (Svelte-escaped) text. Shared between StagingPanel (which passes a stage/unstage action
  // per hunk, plus a sub-hunk line-selection action) and the read-only selected-commit view
  // (`CommitDiffView`), which passes none of it.
  import type { BinaryPreview, FileDiff, Hunk, Line } from "$lib/git/types";
  import { diffViewState } from "./diffViewMode.svelte";
  import { highlightSource, splitHighlightedHtml } from "./highlight";
  import ImageDiff from "./ImageDiff.svelte";
  import { detectLanguage } from "./languages";
  import { isImagePath, imageMimeType } from "./isImagePath";
  import { pairHunkLines } from "./pairHunkLines";
  import { lineCheckboxGroups } from "./lineCheckboxGroups";

  let {
    file,
    hunkActionLabel,
    onHunkAction,
    lineActionLabel,
    onLineAction,
    resolveImagePreview,
  }: {
    file: FileDiff;
    hunkActionLabel?: string;
    onHunkAction?: (hunk: Hunk) => void;
    /** Verb prefix used in each line checkbox's `aria-label`, e.g. "Stage"/"Unstage".
     *  Checking an addition/deletion line's box immediately acts on just that line — no
     *  separate confirmation step — only offered when this and `onLineAction` are both
     *  given, and only in inline view — the side-by-side split's left/right cell layout
     *  doesn't map cleanly onto a single per-line checkbox column, and hunk-level staging
     *  already covers that mode. */
    lineActionLabel?: string;
    onLineAction?: (hunk: Hunk, lineIndices: number[]) => Promise<void>;
    /** Parent-owned, same "caller knows which sides apply here" contract as
     *  `CommitDiffView`'s `onOpenExternalDiff` — only the caller knows whether this diff's
     *  old/new sides are workdir/index/a commit-ish. Only consulted for a binary file whose
     *  path looks like a supported image format; every other binary file keeps the plain
     *  placeholder regardless of whether this prop is given. */
    resolveImagePreview?: (
      file: FileDiff,
    ) => Promise<{ old: BinaryPreview | null; new: BinaryPreview | null }>;
  } = $props();

  const isImageFile = $derived(file.isBinary && isImagePath(file.newPath ?? file.oldPath));

  let imagePreview = $state<{ old: BinaryPreview | null; new: BinaryPreview | null } | null>(
    null,
  );
  let imagePreviewFailed = $state(false);

  // Generation-guarded so switching to a different binary file quickly can't have an
  // earlier fetch's result land after a later one's, matching `+page.svelte`'s `loadDiff`.
  let imagePreviewGeneration = 0;

  $effect(() => {
    const currentFile = file;
    imagePreview = null;
    imagePreviewFailed = false;
    if (!isImageFile || !resolveImagePreview) return;

    const myGeneration = ++imagePreviewGeneration;
    resolveImagePreview(currentFile).then(
      (result) => {
        if (myGeneration === imagePreviewGeneration) imagePreview = result;
      },
      () => {
        if (myGeneration === imagePreviewGeneration) imagePreviewFailed = true;
      },
    );
  });

  // Lines currently mid-flight (checked, action in progress) per hunk — checkbox shows
  // checked-and-disabled for these so a second click can't fire a duplicate action while
  // the first is still applying. Keyed by hunk object identity, which is stable for the
  // lifetime of one `file` prop value; reset via the `$effect` below whenever a fresh diff
  // replaces `file` wholesale (the in-flight line's row won't exist in the new hunks
  // anyway, once staged/unstaged).
  let pendingByHunk = $state(new Map<Hunk, Set<number>>());

  $effect(() => {
    void file;
    pendingByHunk = new Map();
  });

  function isGroupPending(hunk: Hunk, indices: number[]): boolean {
    const pending = pendingByHunk.get(hunk);
    return pending ? indices.some((index) => pending.has(index)) : false;
  }

  function setGroupPending(hunk: Hunk, indices: number[], pending: boolean) {
    const next = new Map(pendingByHunk);
    const set = new Set(next.get(hunk) ?? []);
    for (const index of indices) {
      if (pending) {
        set.add(index);
      } else {
        set.delete(index);
      }
    }
    next.set(hunk, set);
    pendingByHunk = next;
  }

  async function handleLineCheckbox(hunk: Hunk, indices: number[]) {
    if (isGroupPending(hunk, indices)) return;
    setGroupPending(hunk, indices, true);
    try {
      await onLineAction?.(hunk, indices);
    } finally {
      setGroupPending(hunk, indices, false);
    }
  }

  // Per-hunk checkbox grouping — see `lineCheckboxGroups` for why an edited-line
  // replacement (delete+add pair) collapses onto a single checkbox instead of one per line.
  const checkboxGroupsByHunk = $derived.by(() => {
    const map = new Map<Hunk, (number[] | null)[]>();
    for (const hunk of file.hunks) {
      map.set(hunk, lineCheckboxGroups(hunk.lines));
    }
    return map;
  });

  function checkboxGroupFor(hunk: Hunk, index: number): number[] | null {
    return checkboxGroupsByHunk.get(hunk)?.[index] ?? null;
  }

  // Per-hunk, per-line highlighted HTML (index-aligned with that hunk's `lines`); absent for
  // a hunk when the file's extension has no recognized language, falling back to plain text.
  const highlightedByHunk = $derived.by(() => {
    const map = new Map<Hunk, string[]>();
    const language = detectLanguage(file.newPath ?? file.oldPath);
    if (!language) return map;

    for (const hunk of file.hunks) {
      const source = hunk.lines.map((line) => line.content).join("");
      const html = highlightSource(source, language);
      if (html !== null) map.set(hunk, splitHighlightedHtml(html, hunk.lines.length));
    }
    return map;
  });

  function lineText(line: Line): string {
    return line.content.endsWith("\n") ? line.content.slice(0, -1) : line.content;
  }

  function lineHtml(hunk: Hunk, index: number): string | null {
    return highlightedByHunk.get(hunk)?.[index] ?? null;
  }

  function toggleViewMode() {
    diffViewState.mode = diffViewState.mode === "inline" ? "side-by-side" : "inline";
  }
</script>

{#if file.isBinary}
  {#if isImageFile && imagePreview}
    <ImageDiff
      oldPreview={imagePreview.old}
      newPreview={imagePreview.new}
      mimeType={imageMimeType(file.newPath ?? file.oldPath ?? "")}
    />
  {:else if isImageFile && resolveImagePreview && !imagePreviewFailed}
    <p class="placeholder">Loading preview…</p>
  {:else}
    <p class="placeholder">Binary file — no diff to show.</p>
  {/if}
{:else}
  <div class="view-toggle">
    <button type="button" onclick={toggleViewMode}>
      {diffViewState.mode === "inline" ? "Side-by-side view" : "Inline view"}
    </button>
  </div>
  {#each file.hunks as hunk}
    <div class="hunk">
      <div class="hunk-header">
        <span class="hunk-label">{hunk.header}</span>
        <div class="hunk-actions">
          {#if onHunkAction}
            <button type="button" onclick={() => onHunkAction?.(hunk)}>
              {hunkActionLabel}
            </button>
          {/if}
        </div>
      </div>

      <div class="hunk-body">
        {#if diffViewState.mode === "inline"}
          {#each hunk.lines as line, index}
            {@const html = lineHtml(hunk, index)}
            {@const checkboxGroup = checkboxGroupFor(hunk, index)}
            <div
              class="line"
              class:addition={line.origin === "addition"}
              class:deletion={line.origin === "deletion"}
            >
              {#if onLineAction}
                <span class="line-checkbox">
                  {#if checkboxGroup}
                    <input
                      type="checkbox"
                      checked={isGroupPending(hunk, checkboxGroup)}
                      disabled={isGroupPending(hunk, checkboxGroup)}
                      onchange={() => handleLineCheckbox(hunk, checkboxGroup)}
                      aria-label={checkboxGroup.length > 1
                        ? `${lineActionLabel ?? "Toggle"} this edit`
                        : `${lineActionLabel ?? "Toggle"} this ${line.origin} line`}
                    />
                  {/if}
                </span>
              {/if}
              <span class="lineno">{line.oldLineno ?? ""}</span>
              <span class="lineno">{line.newLineno ?? ""}</span>
              <span class="marker"
                >{line.origin === "addition" ? "+" : line.origin === "deletion" ? "-" : " "}</span
              >
              {#if html}
                <span class="content">{@html html}</span>
              {:else}
                <span class="content">{lineText(line)}</span>
              {/if}
            </div>
          {/each}
        {:else}
          {#each pairHunkLines(hunk.lines) as row}
            <div class="split-row">
              <div
                class="split-cell"
                class:empty={!row.left}
                class:deletion={row.left?.line.origin === "deletion"}
              >
                {#if row.left}
                  {@const html = lineHtml(hunk, row.left.index)}
                  <span class="lineno">{row.left.line.oldLineno ?? ""}</span>
                  {#if html}
                    <span class="content">{@html html}</span>
                  {:else}
                    <span class="content">{lineText(row.left.line)}</span>
                  {/if}
                {/if}
              </div>
              <div
                class="split-cell"
                class:empty={!row.right}
                class:addition={row.right?.line.origin === "addition"}
              >
                {#if row.right}
                  {@const html = lineHtml(hunk, row.right.index)}
                  <span class="lineno">{row.right.line.newLineno ?? ""}</span>
                  {#if html}
                    <span class="content">{@html html}</span>
                  {:else}
                    <span class="content">{lineText(row.right.line)}</span>
                  {/if}
                {/if}
              </div>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/each}
{/if}

<style>
  .placeholder {
    color: var(--text-muted);
  }

  .view-toggle {
    display: flex;
    justify-content: flex-end;
    margin-bottom: 0.4rem;
  }

  .view-toggle button {
    font-size: 0.75rem;
    padding: 0.25rem 0.6rem;
    color: var(--text-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .view-toggle button:hover {
    background: var(--surface-2);
  }

  .hunk {
    margin-bottom: 0.75rem;
    font-family: var(--font-mono);
    font-size: 0.8rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .hunk-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: var(--surface-2);
    padding: 0.25rem 0.6rem;
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }

  .hunk-body {
    overflow-x: auto;
    border-radius: 0 0 var(--radius-md) var(--radius-md);
  }

  .hunk-label {
    color: var(--text-muted);
  }

  .hunk-actions {
    display: flex;
    gap: 0.4rem;
  }

  .hunk-header button {
    font-size: 0.7rem;
    padding: 0.15rem 0.45rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .hunk-header button:hover {
    background: var(--surface-0);
  }

  .line {
    display: flex;
    gap: 0.5rem;
    white-space: pre;
  }

  .line-checkbox {
    flex-shrink: 0;
    width: 1rem;
    display: flex;
    align-items: center;
  }

  .line-checkbox input {
    cursor: pointer;
  }

  .line.addition {
    background: var(--success-bg);
  }

  .line.deletion {
    background: var(--danger-bg);
  }

  .lineno {
    flex-shrink: 0;
    width: 2.5rem;
    text-align: right;
    color: var(--text-muted);
  }

  .marker {
    flex-shrink: 0;
    width: 1ch;
  }

  .split-row {
    display: flex;
  }

  .split-cell {
    flex: 1 1 50%;
    display: flex;
    gap: 0.5rem;
    white-space: pre;
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

  /* Syntax token colors, reusing the same Okabe-Ito colorblind-safe hues as the commit
     graph's branch palette (`src/lib/graph/palette.ts`) for a consistent accent set. */
  :global(.hljs-comment),
  :global(.hljs-quote) {
    color: light-dark(#666, #999);
    font-style: italic;
  }

  :global(.hljs-keyword),
  :global(.hljs-selector-tag),
  :global(.hljs-literal),
  :global(.hljs-subst) {
    color: light-dark(#0072b2, #56b4e9);
  }

  :global(.hljs-string),
  :global(.hljs-doctag) {
    color: light-dark(#00785a, #009e73);
  }

  :global(.hljs-number),
  :global(.hljs-symbol) {
    color: light-dark(#a3390a, #d55e00);
  }

  :global(.hljs-title),
  :global(.hljs-section),
  :global(.hljs-selector-id) {
    color: light-dark(#8a7300, #f0e442);
  }

  :global(.hljs-type) {
    color: light-dark(#a0447d, #cc79a7);
  }

  :global(.hljs-attr),
  :global(.hljs-variable),
  :global(.hljs-template-variable),
  :global(.hljs-attribute) {
    color: light-dark(#00539c, #5fa8e0);
  }

  :global(.hljs-built_in),
  :global(.hljs-builtin-name) {
    color: light-dark(#00787a, #00c2c4);
  }

  :global(.hljs-meta) {
    color: light-dark(#666, #aaa);
  }

  :global(.hljs-emphasis) {
    font-style: italic;
  }

  :global(.hljs-strong) {
    font-weight: bold;
  }
</style>
