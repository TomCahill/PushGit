<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Toolbar sitting above the commit graph: branches/tags/stashes now live here as
  // popovers (`Popover.svelte`) instead of always-on sections in a sidebar, so the graph
  // gets the vertical space it deserves and the left rail (`RepoRail.svelte`) is free to be
  // just "which repo" rather than "which repo + everything about it". None of
  // `BranchSidebar`/`TagsPanel`/`StashPanel`/`RemotePanel` know they're inside a popover —
  // this just hosts them and reads back a couple of summary values (current branch name,
  // tag/stash counts) to show on the trigger buttons themselves.
  import BranchSidebar from "$lib/branch/BranchSidebar.svelte";
  import MaintenancePanel from "$lib/maintenance/MaintenancePanel.svelte";
  import RemotePanel from "$lib/remote/RemotePanel.svelte";
  import StashPanel from "$lib/stash/StashPanel.svelte";
  import TagsPanel from "$lib/tags/TagsPanel.svelte";
  import UndoRedoControls from "$lib/undo/UndoRedoControls.svelte";
  import WorkflowPanel from "$lib/workflow/WorkflowPanel.svelte";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import type { RebaseCommitSummary } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onConflicts,
    onApplied,
    onSearchChange,
    onInteractiveRebase,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged: () => void;
    onConflicts: () => void;
    onApplied: () => void;
    /** Fires on every keystroke — the parent is responsible for debouncing before it
     *  actually triggers a graph refetch ("live as you type"). */
    onSearchChange: (query: string) => void;
    onInteractiveRebase?: (onto: string, commits: RebaseCommitSummary[]) => void;
  } = $props();

  let currentBranch = $state<string | null>(null);
  let tagCount = $state(0);
  let stashCount = $state(0);
  let searchQuery = $state("");

  function handleSearchInput(event: Event & { currentTarget: HTMLInputElement }) {
    searchQuery = event.currentTarget.value;
    onSearchChange(searchQuery);
  }
</script>

<div class="action-rail">
  <Popover>
    {#snippet trigger()}
      <Icon name="branch" size={13} />
      <span class="trigger-label">{currentBranch ?? "Branches"}</span>
      <Icon name="chevron-down" size={12} />
    {/snippet}
    {#snippet children()}
      <BranchSidebar
        {repoPath}
        {refreshKey}
        {onChanged}
        {onConflicts}
        onCurrentBranchChange={(name) => (currentBranch = name)}
        {onInteractiveRebase}
      />
    {/snippet}
  </Popover>

  <Popover>
    {#snippet trigger()}
      <Icon name="tag" size={13} />
      <span class="trigger-label">Tags{tagCount > 0 ? ` (${tagCount})` : ""}</span>
      <Icon name="chevron-down" size={12} />
    {/snippet}
    {#snippet children()}
      <TagsPanel {repoPath} {refreshKey} {onChanged} onCountChange={(n) => (tagCount = n)} />
    {/snippet}
  </Popover>

  <Popover>
    {#snippet trigger()}
      <Icon name="archive" size={13} />
      <span class="trigger-label">Stash{stashCount > 0 ? ` (${stashCount})` : ""}</span>
      <Icon name="chevron-down" size={12} />
    {/snippet}
    {#snippet children()}
      <StashPanel
        {repoPath}
        {refreshKey}
        {onChanged}
        {onApplied}
        onCountChange={(n) => (stashCount = n)}
      />
    {/snippet}
  </Popover>

  <Popover>
    {#snippet trigger()}
      <Icon name="wrench" size={13} />
      <span class="trigger-label">Maintenance</span>
      <Icon name="chevron-down" size={12} />
    {/snippet}
    {#snippet children()}
      <MaintenancePanel {repoPath} {refreshKey} />
    {/snippet}
  </Popover>

  <Popover>
    {#snippet trigger()}
      <Icon name="refresh-cw" size={13} />
      <span class="trigger-label">Workflow</span>
      <Icon name="chevron-down" size={12} />
    {/snippet}
    {#snippet children()}
      <WorkflowPanel {repoPath} {refreshKey} {onChanged} {onConflicts} />
    {/snippet}
  </Popover>

  <UndoRedoControls {repoPath} {refreshKey} {onChanged} />

  <input
    type="text"
    class="search-box"
    value={searchQuery}
    oninput={handleSearchInput}
    placeholder="Search commits, authors, SHA, or refs…"
    aria-label="Search commit graph"
  />

  <RemotePanel {repoPath} {refreshKey} {onChanged} {onConflicts} />
</div>

<style>
  .action-rail {
    display: flex;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0.6rem 0.75rem;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
  }

  .search-box {
    flex: 1 1 auto;
    min-width: 8rem;
    font: inherit;
    font-size: 0.8rem;
    padding: 0.35rem 0.6rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .search-box:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .trigger-label {
    max-width: 10rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
