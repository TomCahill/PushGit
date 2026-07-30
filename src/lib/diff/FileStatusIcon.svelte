<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Colored glyph replacing the old "ADDED"/"MODIFIED"/... text label in a file row
  // (`StagingPanel.svelte`, `CommitDiffView.svelte`). The status is still exposed via
  // `title`/`aria-label` for tooltips and screen readers.
  import Icon, { type IconName } from "$lib/shell/Icon.svelte";
  import type { FileStatus } from "$lib/git/types";

  let { status }: { status: FileStatus } = $props();

  const info: Record<FileStatus, { icon: IconName; label: string; class: string }> = {
    added: { icon: "plus", label: "Added", class: "added" },
    deleted: { icon: "minus", label: "Deleted", class: "deleted" },
    modified: { icon: "edit-2", label: "Modified", class: "modified" },
    renamed: { icon: "arrow-right", label: "Renamed", class: "renamed" },
    copied: { icon: "copy", label: "Copied", class: "renamed" },
    typechange: { icon: "refresh-cw", label: "Type changed", class: "muted" },
    conflicted: { icon: "alert-triangle", label: "Conflicted", class: "conflicted" },
    untracked: { icon: "help-circle", label: "Untracked", class: "muted" },
    unreadable: { icon: "alert-triangle", label: "Unreadable", class: "muted" },
  };

  const current = $derived(info[status]);
</script>

<span class="file-status-icon {current.class}" title={current.label} aria-label={current.label}>
  <Icon name={current.icon} size={14} />
</span>

<style>
  .file-status-icon {
    display: inline-flex;
    flex-shrink: 0;
  }

  .added {
    color: var(--success);
  }

  .deleted,
  .conflicted {
    color: var(--danger);
  }

  .modified {
    color: var(--warning);
  }

  .renamed {
    color: var(--accent);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
