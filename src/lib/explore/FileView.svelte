<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  import { untrack } from "svelte";
  import {
    diffBetweenCommits,
    diffCommitToWorkdir,
    fileHistory,
    readBlobAt,
    refStatus,
    resolveRev,
  } from "$lib/git/api";
  import type { BlobView, FileDiff, FileDiffSelection, RefStatus } from "$lib/git/types";
  import Button from "$lib/shell/Button.svelte";
  import CopyButton from "$lib/shell/CopyButton.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import { highlightSource } from "$lib/diff/highlight";
  import { detectLanguage } from "$lib/diff/languages";
  import { imageMimeType, isImagePath } from "$lib/diff/isImagePath";
  import { browse, exploreState, shortOid, sourceKey } from "./explore.svelte";
  import RefCombobox from "./RefCombobox.svelte";

  let {
    repoPath,
    refreshKey,
    watchActive,
    pickerOpen = $bindable(false),
    onBack,
    onRefresh,
    onShowDiff,
    onBlame,
    onResolveConflict,
    onOpenSubmodule,
  }: {
    repoPath: string;
    refreshKey: number;
    watchActive: boolean;
    pickerOpen?: boolean;
    onBack: () => void;
    onRefresh: () => void;
    onShowDiff: (diff: FileDiffSelection) => void;
    onBlame: (path: string, rev: string | null) => void;
    onResolveConflict: (path: string) => void;
    onOpenSubmodule: (path: string) => void;
  } = $props();

  // Plain text past this many lines: highlighting a huge file would stall the UI.
  const MAX_HIGHLIGHT_LINES = 10_000;
  const LFS_POINTER_PREFIX = "version https://git-lfs.github.com/spec/v1";

  let view = $state<BlobView | null>(null);
  let loadError = $state<string | null>(null);
  let actionNote = $state<string | null>(null);
  let moved = $state<RefStatus>({ kind: "unchanged" });
  let generation = 0;

  const source = $derived(exploreState.source);
  const path = $derived(exploreState.path);
  const status = $derived(exploreState.status);
  const isWorkingTree = $derived(source.kind === "workingTree");
  const revOid = $derived(source.kind === "revision" ? source.oid : null);

  async function load() {
    const gen = ++generation;
    const target = exploreState.path;
    if (!target) {
      view = null;
      return;
    }
    try {
      const result = await readBlobAt(repoPath, exploreState.source, target, isImagePath(target));
      if (gen !== generation) return;
      view = result;
      loadError = null;
    } catch (err) {
      if (gen === generation) loadError = String(err);
    }
  }

  $effect(() => {
    void sourceKey(source);
    void path;
    untrack(() => {
      view = null;
      loadError = null;
      actionNote = null;
      void load();
    });
  });

  let lastRefresh = untrack(() => refreshKey);
  $effect(() => {
    if (refreshKey === lastRefresh) return;
    lastRefresh = refreshKey;
    untrack(() => {
      if (exploreState.source.kind === "workingTree") void load();
    });
  });

  $effect(() => {
    void refreshKey;
    const label = exploreState.refLabel;
    const oid = revOid;
    moved = { kind: "unchanged" };
    if (!label || !oid) return;
    void refStatus(repoPath, label, oid).then((result) => {
      if (exploreState.refLabel !== label || revOid !== oid) return;
      if (result.kind === "gone") exploreState.refLabel = null;
      else moved = result;
    });
  });

  function refreshPinned() {
    if (moved.kind !== "moved") return;
    const path = exploreState.path;
    browse({ kind: "revision", oid: moved.oid }, exploreState.refLabel, path);
  }

  const crumbs = $derived.by(() => {
    if (!path) return [];
    const parts = path.split("/");
    return parts.map((name, i) => ({ name, path: parts.slice(0, i + 1).join("/") }));
  });

  const repoName = $derived(repoPath.replace(/\/+$/, "").split("/").pop() ?? repoPath);

  const text = $derived(view?.kind === "text" ? view.content : null);
  const lineCount = $derived(
    text === null ? 0 : text.split("\n").length - (text.endsWith("\n") ? 1 : 0),
  );
  const highlighted = $derived.by(() => {
    if (text === null || !path || lineCount > MAX_HIGHLIGHT_LINES) return null;
    const language = detectLanguage(path);
    return language ? highlightSource(text, language) : null;
  });
  const lineNumbers = $derived(
    Array.from({ length: Math.max(lineCount, 1) }, (_, i) => i + 1).join("\n"),
  );

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  function findFile(files: FileDiff[], target: string): FileDiff | undefined {
    return files.find((f) => f.newPath === target || f.oldPath === target);
  }

  async function runAction(action: () => Promise<void>) {
    actionNote = null;
    try {
      await action();
    } catch (err) {
      actionNote = String(err);
    }
  }

  const diffVsHead = () =>
    runAction(async () => {
      if (!path) return;
      const file = findFile(await diffCommitToWorkdir(repoPath, "HEAD"), path);
      if (file) onShowDiff({ file });
      else actionNote = "No changes from HEAD.";
    });

  const diffVsWorkingTree = () =>
    runAction(async () => {
      if (!path || !revOid) return;
      const file = findFile(await diffCommitToWorkdir(repoPath, revOid), path);
      if (file) onShowDiff({ file });
      else actionNote = "Same as the working tree.";
    });

  // shortcut: looks up the previous version on click instead of disabling the button up front, to skip a history walk per file opened.
  const diffVsPrevious = () =>
    runAction(async () => {
      if (!path || !revOid) return;
      const history = await fileHistory(repoPath, path, revOid);
      if (history.length < 2) {
        actionNote = "This is the first version of the file.";
        return;
      }
      const file = findFile(
        await diffBetweenCommits(repoPath, history[1].oid, history[0].oid),
        path,
      );
      if (file) onShowDiff({ file });
    });

  const viewAtHead = () =>
    runAction(async () => {
      browse({ kind: "revision", oid: await resolveRev(repoPath, "HEAD") }, "HEAD", path);
    });

  const notInHead = $derived(status === "untracked" || status === "added");
</script>

<section class="file-view" aria-label="Browse files">
  <div class="header">
    <div class="row">
      <nav class="crumbs" aria-label="Path">
        <span class="current root">{repoName}</span>
        {#each crumbs as crumb, i (crumb.path)}
          <span class="sep">/</span>
          {#if i === crumbs.length - 1}
            <span class="current" aria-current="page">{crumb.name}</span>
          {:else}
            <button
              type="button"
              class="crumb"
              onclick={() => (exploreState.reveal = `${crumb.path}/`)}>{crumb.name}</button
            >
          {/if}
        {/each}
      </nav>
      <Button variant="text" onclick={onBack}>← Back to graph</Button>
    </div>

    <div class="row">
      <RefCombobox
        {repoPath}
        {source}
        refLabel={exploreState.refLabel}
        bind:open={pickerOpen}
        onPick={(next, label) => browse(next, label, exploreState.path)}
      />
      {#if isWorkingTree}
        {#if watchActive}
          <span class="chip ok"><Icon name="refresh-cw" size={12} /> Live</span>
        {:else}
          <span class="chip plain"><Icon name="eye-off" size={12} /> Not watching</span>
          <Button variant="tonal" onclick={onRefresh}>Refresh</Button>
        {/if}
      {:else if moved.kind === "moved"}
        <span class="chip warn">
          <Icon name="alert-triangle" size={12} />
          {exploreState.refLabel}
          {moved.newerCommits === null
            ? "was rewritten"
            : `has moved · ${moved.newerCommits} newer commit${moved.newerCommits === 1 ? "" : "s"}`}
        </span>
        <Button variant="tonal" onclick={refreshPinned}>Refresh</Button>
      {/if}
      {#if status === "modified" || status === "conflicted" || status === "deleted"}
        <span class="chip" class:warn={status === "modified"} class:bad={status !== "modified"}
          >{status === "modified"
            ? "Modified"
            : status === "deleted"
              ? "Deleted"
              : "Conflicted"}</span
        >
      {:else if status === "untracked"}
        <span class="chip plain">Untracked</span>
      {/if}
    </div>

    {#if path}
      <div class="row actions">
        {#if isWorkingTree}
          <Button
            variant="tonal"
            onclick={diffVsHead}
            disabled={!status || status === "untracked"}
            title={!status
              ? "No changes from HEAD"
              : status === "untracked"
                ? "Not in HEAD yet"
                : undefined}>Diff vs HEAD</Button
          >
          <Button
            variant="outlined"
            onclick={() => onBlame(path, null)}
            disabled={notInHead}
            title={notInHead ? "Not in any commit yet, so there's no history" : undefined}
            >Blame &amp; history at HEAD</Button
          >
          <CopyButton text={path} label={`Copy path ${path}`} />
        {:else if revOid}
          <Button variant="outlined" onclick={() => onBlame(path, revOid)}
            >Blame &amp; history</Button
          >
          <Button variant="outlined" onclick={diffVsPrevious}>Diff vs previous version</Button>
          <Button variant="outlined" onclick={diffVsWorkingTree}>Diff vs working tree</Button>
          <CopyButton
            text={`${path}@${shortOid(revOid)}`}
            label={`Copy ${path}@${shortOid(revOid)}`}
          />
        {/if}
      </div>
      {#if actionNote}
        <p class="note" role="status">{actionNote}</p>
      {/if}
    {/if}
  </div>

  <div class="body">
    {#if !path}
      <p class="empty">
        <Icon name="file" size={40} />
        <span>Pick a file from the tree, or press <kbd>Ctrl P</kbd></span>
      </p>
    {:else if loadError}
      <p class="error" role="alert">Couldn't read {path}: {loadError}</p>
    {:else if !view}
      <p class="placeholder">Loading {path}…</p>
    {:else if view.kind === "text"}
      {#if view.truncated}
        <p class="banner">
          Showing the first {formatBytes(view.content.length)} of {formatBytes(view.byteLen)}.
        </p>
      {/if}
      {#if view.content.startsWith(LFS_POINTER_PREFIX)}
        <p class="banner info">
          This is a Git LFS pointer. The real file is stored on the LFS server.
        </p>
      {/if}
      {#if status === "conflicted"}
        <p class="banner bad">
          Merge in progress: this file has conflicts.
          <Button variant="filled" onclick={() => onResolveConflict(path)}>Resolve…</Button>
        </p>
      {/if}
      <div class="code" aria-label={path} aria-readonly="true">
        <pre class="numbers" aria-hidden="true">{lineNumbers}</pre>
        {#if highlighted !== null}
          <pre class="source">{@html highlighted}</pre>
        {:else}
          <pre class="source">{view.content}</pre>
        {/if}
      </div>
    {:else if view.kind === "image"}
      {#if view.preview.kind === "content"}
        <div class="image-wrap">
          <img src="data:{imageMimeType(path)};base64,{view.preview.base64}" alt={path} />
        </div>
        <p class="meta">{formatBytes(view.preview.byteLen)}</p>
      {:else}
        <p class="empty">
          <span>Image too large to preview ({formatBytes(view.preview.byteLen)}).</span>
        </p>
      {/if}
    {:else if view.kind === "binary"}
      <p class="empty">
        <Icon name="file" size={40} />
        <strong>Binary file, {formatBytes(view.byteLen)}</strong>
        <span>Can't be shown as text.</span>
      </p>
    {:else if view.kind === "symlink"}
      <p class="empty">
        <Icon name="link" size={40} />
        <strong>Symbolic link</strong>
        <code>→ {view.target}</code>
        <span>Links aren't followed, so a link can never show a file from outside the repo.</span>
      </p>
    {:else if view.kind === "submodule"}
      <p class="empty">
        <Icon name="folder" size={40} />
        <strong>Submodule pinned at <code>{shortOid(view.commit)}</code></strong>
        <span>Its files are in its own repository.</span>
        <Button variant="tonal" onclick={() => onOpenSubmodule(path)}>Open submodule</Button>
      </p>
    {:else if view.kind === "deleted"}
      <p class="empty">
        <Icon name="minus" size={40} />
        <strong>Deleted from disk</strong>
        <span>It's still in HEAD, so the tree lists it struck through.</span>
        <Button variant="outlined" onclick={viewAtHead}>View at HEAD</Button>
      </p>
    {/if}
  </div>
</section>

<style>
  .file-view {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }

  .header {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: var(--space-4) var(--space-4) 0.75rem;
    border-bottom: 1px solid var(--border);
  }

  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    min-width: 0;
  }

  .crumbs {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 0.15rem;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .crumb {
    padding: 0.1rem 0.25rem;
    font: inherit;
    color: inherit;
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
  }

  .crumb:hover {
    background: var(--surface-2);
  }

  .crumb:focus-visible {
    outline: 2px solid var(--accent);
  }

  .current {
    padding: 0.1rem 0.25rem;
    color: var(--text-primary);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0.65rem;
    border-radius: 1rem;
    font-size: 0.78rem;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .chip.ok {
    background: var(--success-bg);
  }

  .chip.warn {
    background: var(--warning-bg);
  }

  .chip.bad {
    background: var(--danger-bg);
  }

  .chip.plain {
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .note {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }

  .code {
    display: flex;
    align-items: flex-start;
    font-family: var(--font-mono);
    font-size: 0.8rem;
    line-height: 1.55;
  }

  pre {
    margin: 0;
    padding: 0.5rem 0;
    font: inherit;
  }

  .numbers {
    flex-shrink: 0;
    min-width: 3rem;
    padding-right: 1rem;
    text-align: right;
    color: var(--text-secondary);
    user-select: none;
  }

  .source {
    flex: 1 1 auto;
    padding-right: 1rem;
    color: var(--text-primary);
  }

  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    padding: 0.5rem var(--space-4);
    font-size: 0.8rem;
    color: var(--text-primary);
    background: var(--warning-bg);
  }

  .banner.info {
    background: var(--accent-bg);
  }

  .banner.bad {
    background: var(--danger-bg);
  }

  .empty {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.6rem;
    margin: 0;
    padding: var(--space-5);
    text-align: center;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .empty strong {
    color: var(--text-primary);
    font-weight: 500;
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.78rem;
    padding: 0.3rem 0.6rem;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--text-primary);
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    padding: 0 0.35rem;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .image-wrap {
    padding: var(--space-4);
  }

  .image-wrap img {
    max-width: 100%;
    background-color: var(--surface-0);
    background-image:
      linear-gradient(
        45deg,
        var(--surface-2) 25%,
        transparent 25%,
        transparent 75%,
        var(--surface-2) 75%
      ),
      linear-gradient(
        45deg,
        var(--surface-2) 25%,
        transparent 25%,
        transparent 75%,
        var(--surface-2) 75%
      );
    background-size: 16px 16px;
    background-position:
      0 0,
      8px 8px;
  }

  .meta,
  .placeholder {
    margin: 0;
    padding: 0 var(--space-4) var(--space-4);
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  .placeholder {
    padding-top: var(--space-4);
  }

  .error {
    margin: 0;
    padding: var(--space-4);
    font-size: 0.85rem;
    color: var(--danger);
  }
</style>
