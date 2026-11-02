<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  import Button from "$lib/shell/Button.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import Logo from "$lib/shell/Logo.svelte";
  import { formatRelativeTime } from "$lib/shell/relativeTime";
  import { repoDisplayName, type RecentRepo } from "./recentRepos";
  import type { RepoFolder } from "./repoFolders";

  const RECENT_COUNT = 5;

  let {
    recentRepos,
    folders,
    openError,
    onOpen,
    onSelectRecent,
  }: {
    recentRepos: RecentRepo[];
    folders: RepoFolder[];
    openError: string | null;
    onOpen: () => void;
    onSelectRecent: (path: string) => void;
  } = $props();

  const recent = $derived(
    [...recentRepos].sort((a, b) => b.lastOpenedAt - a.lastOpenedAt).slice(0, RECENT_COUNT),
  );

  function folderName(folderId: string | undefined): string | undefined {
    return folders.find((folder) => folder.id === folderId)?.name;
  }
</script>

<div class="welcome">
  <div class="card">
    <div class="heading">
      <span class="mark"><Logo size={20} /></span>
      <h2>Open a repository</h2>
    </div>
    <p class="lead">
      Pick a folder that contains a git repository, or jump back into a recent one.
    </p>

    <!-- No title here: the rail's Open button owns "Open a repository", which tests look up by title. -->
    <div class="open-row">
      <Button onclick={onOpen}>
        <Icon name="folder-open" size={14} /> Open repository
      </Button>
      <kbd>Ctrl+O</kbd>
    </div>

    {#if openError}
      <p class="error" role="alert">{openError}</p>
    {/if}

    {#if recent.length > 0}
      <h3>Recent</h3>
      <ul aria-label="Recent repositories">
        {#each recent as entry (entry.path)}
          {@const folder = folderName(entry.folderId)}
          <li>
            <button type="button" title={entry.path} onclick={() => onSelectRecent(entry.path)}>
              <Icon name="branch" size={13} />
              <span class="name">{repoDisplayName(entry.path)}</span>
              <span class="meta">
                {folder ? `${folder} · ` : ""}{formatRelativeTime(entry.lastOpenedAt)}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .welcome {
    display: flex;
    justify-content: center;
    align-items: flex-start;
    height: 100%;
    padding: 12vh 1.5rem 1.5rem;
    overflow: auto;
    box-sizing: border-box;
  }

  .card {
    width: min(26rem, 100%);
  }

  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .mark {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    color: var(--on-primary-container);
    background: var(--primary-container);
    border-radius: 24%;
  }

  h2 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 500;
    color: var(--text-primary);
  }

  .lead {
    margin: 0.5rem 0 1rem;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .open-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  kbd {
    padding: 0.1rem 0.35rem;
    font-family: inherit;
    font-size: 0.72rem;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .error {
    margin: 0.75rem 0 0;
    font-size: 0.8rem;
    color: var(--danger);
  }

  h3 {
    margin: 1.5rem 0 0.35rem 0.5rem;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li button {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    padding: 0.45rem 0.5rem;
    font: inherit;
    font-size: 0.85rem;
    color: var(--text-primary);
    text-align: left;
    background: none;
    border: none;
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  li button:hover {
    background: var(--surface-2);
  }

  li button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    flex-shrink: 0;
    margin-left: auto;
    font-size: 0.75rem;
    color: var(--text-muted);
  }
</style>
