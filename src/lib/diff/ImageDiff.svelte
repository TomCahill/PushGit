<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders a before/after comparison for one image file's two sides — used by `HunkDiff`
  // (a working-directory/staged/commit/compare diff) and `ConflictEditor` (a binary merge
  // conflict). Each side is `null` when that side doesn't exist at all (an added or deleted
  // file), independent of `BinaryPreview`'s own content-vs-too-large distinction for a side
  // that *does* exist. See `.private/feature/image-binary-diff/PLAN.md`.
  import type { BinaryPreview } from "$lib/git/types";

  let {
    oldPreview,
    newPreview,
    mimeType,
    oldLabel = "Before",
    newLabel = "After",
  }: {
    oldPreview: BinaryPreview | null;
    newPreview: BinaryPreview | null;
    mimeType: string;
    oldLabel?: string;
    newLabel?: string;
  } = $props();

  let oldDimensions = $state<{ width: number; height: number } | null>(null);
  let newDimensions = $state<{ width: number; height: number } | null>(null);

  $effect(() => {
    void oldPreview;
    oldDimensions = null;
  });
  $effect(() => {
    void newPreview;
    newDimensions = null;
  });

  function dataUri(base64: string): string {
    return `data:${mimeType};base64,${base64}`;
  }

  function formatBytes(byteLen: number): string {
    if (byteLen < 1024) return `${byteLen} B`;
    const units = ["KB", "MB", "GB"];
    let value = byteLen / 1024;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    return `${value.toFixed(1)} ${units[unitIndex]}`;
  }

  function handleLoad(event: Event, side: "old" | "new") {
    const img = event.currentTarget as HTMLImageElement;
    const dimensions = { width: img.naturalWidth, height: img.naturalHeight };
    if (side === "old") {
      oldDimensions = dimensions;
    } else {
      newDimensions = dimensions;
    }
  }
</script>

{#snippet column(preview: BinaryPreview | null, label: string, dimensions: { width: number; height: number } | null, side: "old" | "new")}
  <div class="column">
    <div class="column-label">{label}</div>
    {#if preview === null}
      <p class="placeholder">No file</p>
    {:else if preview.kind === "tooLarge"}
      <p class="placeholder">{formatBytes(preview.byteLen)} — too large to preview</p>
    {:else}
      <img
        src={dataUri(preview.base64)}
        alt={label}
        onload={(event) => handleLoad(event, side)}
      />
      <p class="caption">
        {#if dimensions}
          {dimensions.width} × {dimensions.height} ·
        {/if}
        {formatBytes(preview.byteLen)}
      </p>
    {/if}
  </div>
{/snippet}

<div class="image-diff">
  {@render column(oldPreview, oldLabel, oldDimensions, "old")}
  {@render column(newPreview, newLabel, newDimensions, "new")}
</div>

<style>
  .image-diff {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
  }

  .column {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.4rem;
    padding: 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-1);
  }

  .column-label {
    align-self: flex-start;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .column img {
    max-width: 100%;
    max-height: 320px;
    object-fit: contain;
    background: var(--surface-0);
    border-radius: var(--radius-sm);
  }

  .caption {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .placeholder {
    color: var(--text-muted);
  }
</style>
