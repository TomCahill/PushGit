<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Shared "+N -M" insertion/deletion readout — used inline in a file row and as a
  // section-header total (`StagingPanel.svelte`, `CommitDiffView.svelte`). Renders nothing
  // for a binary file or a file with no line-level changes (both report 0/0).
  let { insertions, deletions }: { insertions: number; deletions: number } = $props();
</script>

{#if insertions > 0 || deletions > 0}
  <span class="diff-stat" title="{insertions} insertion(s), {deletions} deletion(s)">
    {#if insertions > 0}<span class="ins">+{insertions}</span>{/if}
    {#if deletions > 0}<span class="del">-{deletions}</span>{/if}
  </span>
{/if}

<style>
  .diff-stat {
    display: inline-flex;
    flex-shrink: 0;
    gap: 0.35rem;
    font-family: var(--font-mono);
    font-size: 0.72rem;
    font-weight: 500;
  }

  .ins {
    color: var(--success);
  }

  .del {
    color: var(--danger);
  }
</style>
