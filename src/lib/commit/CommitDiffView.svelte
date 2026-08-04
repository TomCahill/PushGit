<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Read-only counterpart to StagingPanel's file list, for a selected commit (as opposed to
  // the working directory) — the "hunk-level diff rendering for a selected
  // commit" feature. Only lists the changed files; the shell (`+page.svelte`) renders `HunkDiff`
  // for whichever one is selected in its center panel. There's no stage/unstage action to
  // wire up here since a committed change can't be staged.
  import DiffStat from "$lib/diff/DiffStat.svelte";
  import FileStatusIcon from "$lib/diff/FileStatusIcon.svelte";
  import CopyButton from "$lib/shell/CopyButton.svelte";
  import type { FileDiff } from "$lib/git/types";

  let {
    files,
    selectedPath = $bindable(null),
  }: { files: FileDiff[]; selectedPath?: string | null } = $props();

  function fileKey(file: FileDiff): string {
    return file.newPath ?? file.oldPath ?? "";
  }

  function fileLabel(file: FileDiff): string {
    if (
      (file.status === "renamed" || file.status === "copied") &&
      file.oldPath &&
      file.newPath &&
      file.oldPath !== file.newPath
    ) {
      return `${file.oldPath} → ${file.newPath}`;
    }
    return fileKey(file);
  }

  const totalStats = $derived(
    files.reduce(
      (acc, f) => ({
        insertions: acc.insertions + f.insertions,
        deletions: acc.deletions + f.deletions,
      }),
      { insertions: 0, deletions: 0 },
    ),
  );
</script>

<div class="commit-diff">
  <div class="file-list-header">
    <span>{files.length} file(s) changed</span>
    <DiffStat insertions={totalStats.insertions} deletions={totalStats.deletions} />
  </div>
  <ul class="file-list" aria-label="Changed files">
    {#each files as file (fileKey(file))}
      <li class:selected={selectedPath === fileKey(file)}>
        <button type="button" class="file-row" onclick={() => (selectedPath = fileKey(file))}>
          <FileStatusIcon status={file.status} />
          <span class="path">{fileLabel(file)}</span>
          <DiffStat insertions={file.insertions} deletions={file.deletions} />
        </button>
        <CopyButton text={fileKey(file)} label={`Copy path ${fileKey(file)}`} />
      </li>
    {/each}
  </ul>
</div>

<style>
  .commit-diff {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    height: 100%;
  }

  .file-list-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-muted);
  }

  .file-list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .file-list li {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    border-radius: var(--radius-sm);
  }

  .file-list li:hover {
    background: var(--surface-2);
  }

  .file-list li.selected,
  .file-list li.selected:hover {
    background: var(--accent-bg);
  }

  .file-row {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    gap: 0.5rem;
    align-items: center;
    background: none;
    border: none;
    padding: 0.25rem 0.4rem;
    font: inherit;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
  }

  .path {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
