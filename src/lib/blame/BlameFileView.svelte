<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // The annotated-file half of blame (`BlameView.svelte` owns the history list in the
  // sidebar) — rendered in the shell's center panel, in place of the commit graph, the same
  // way a selected file's diff is. Syntax-highlighted the same way `HunkDiff.svelte` is
  // (`../diff/highlight.ts` + `../diff/languages.ts`, keyed off `path`'s extension); the
  // `hljs-*` token colors it relies on are declared globally by `HunkDiff.svelte` itself, so
  // nothing needs duplicating here. Falls back to plain text for an unrecognized extension.
  import type { BlameLine } from "$lib/git/types";
  import { highlightSource, splitHighlightedHtml } from "../diff/highlight";
  import { detectLanguage } from "../diff/languages";

  let { lines, path }: { lines: BlameLine[]; path: string } = $props();

  // Index-aligned with `lines`; `null` when `path`'s extension has no recognized language, in
  // which case each line falls back to its own plain (Svelte-escaped) content below.
  const highlightedLines = $derived.by(() => {
    const language = detectLanguage(path);
    if (!language) return null;
    const source = lines.map((line) => line.content).join("\n");
    const html = highlightSource(source, language);
    return html !== null ? splitHighlightedHtml(html, lines.length) : null;
  });
</script>

<ul class="blame-lines" aria-label="Blame">
  {#each lines as line, index (line.lineNo)}
    <li>
      <span class="gutter" title={`${line.summary} — ${line.authorName}`}>
        <span class="short-oid">{line.shortOid}</span>
        <span class="author">{line.authorName}</span>
      </span>
      <span class="line-no">{line.lineNo}</span>
      {#if highlightedLines}
        <span class="content">{@html highlightedLines[index]}</span>
      {:else}
        <span class="content">{line.content}</span>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .blame-lines {
    margin: 0;
    padding: 0;
    list-style: none;
    font-family: var(--font-mono);
    font-size: 0.8rem;
  }

  .blame-lines li {
    display: flex;
    gap: 0.6rem;
  }

  .blame-lines li:hover {
    background: var(--surface-1);
  }

  .gutter {
    flex-shrink: 0;
    display: flex;
    gap: 0.4rem;
    width: 14rem;
    overflow: hidden;
    color: var(--text-muted);
    border-right: 1px solid var(--border);
    padding-right: 0.5rem;
  }

  .gutter .short-oid {
    flex-shrink: 0;
  }

  .gutter .author {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line-no {
    flex-shrink: 0;
    width: 2.5rem;
    text-align: right;
    color: var(--text-muted);
  }

  .content {
    white-space: pre;
  }
</style>
