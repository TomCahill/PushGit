<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Left rail: open a repo via a native folder picker, and switch between recently-opened
  // ones — optionally organized into folders. Purely a switcher/organizer now — branch/tag/
  // stash/remote actions for the *currently* open repo live in `ActionRail.svelte` above the
  // graph instead (this used to be one long sidebar with everything in it). `+page.svelte`
  // owns `openRepo()` and all folder/recent-repo persistence (`./recentRepos.ts`,
  // `./repoFolders.ts`) — this component just renders the list/folders and reports
  // clicks/drags/menu selections.
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { pickRepositoryFolder } from "$lib/git/api";
  import { confirmAsync, promptAsync } from "$lib/shell/confirmDialog.svelte";
  import { openContextMenu, type ContextMenuItem } from "$lib/shell/contextMenu.svelte";
  import Button from "$lib/shell/Button.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import Logo from "$lib/shell/Logo.svelte";
  import NotificationsPanel from "$lib/shell/NotificationsPanel.svelte";
  import Popover from "$lib/shell/Popover.svelte";
  import { unreadNotificationCount } from "$lib/shell/toast.svelte";
  import { dismiss as dismissUpdateBanner, updateCheckState } from "$lib/shell/updateCheck.svelte";
  import { repoDisplayName, type RecentRepo } from "./recentRepos";
  import type { RepoFolder } from "./repoFolders";

  // A plain `<a target="_blank">` doesn't open the system browser from inside a Tauri
  // webview — same `preventDefault` + opener-plugin pattern as `AboutDialog.svelte`'s Ko-fi link.
  function handleViewRelease() {
    if (updateCheckState.available) void openUrl(updateCheckState.available.url);
  }

  let appVersion = $state("");
  onMount(() => {
    getVersion()
      .then((version) => (appVersion = version))
      .catch(() => {});
  });

  let {
    currentPath,
    recentRepos,
    folders = [],
    openError,
    onOpenPath,
    onSelectRecent,
    onRemoveRecent,
    onCreateFolder,
    onRenameFolder,
    onDeleteFolder,
    onToggleFolderCollapsed,
    onMoveRepo,
    onMoveFolder,
    onOpenSettings,
  }: {
    currentPath: string;
    recentRepos: RecentRepo[];
    folders?: RepoFolder[];
    openError: string | null;
    onOpenPath: (path: string) => void;
    onSelectRecent: (path: string) => void;
    onRemoveRecent: (path: string) => void;
    onCreateFolder: (name: string) => RepoFolder;
    onRenameFolder: (id: string, name: string) => void;
    onDeleteFolder: (id: string) => void;
    onToggleFolderCollapsed: (id: string) => void;
    onMoveRepo: (path: string, folderId: string | undefined, beforePath: string | null) => void;
    onMoveFolder: (id: string, beforeId: string | null) => void;
    onOpenSettings: () => void;
  } = $props();

  const ungroupedRepos = $derived(recentRepos.filter((entry) => !entry.folderId));
  function reposInFolder(folderId: string): RecentRepo[] {
    return recentRepos.filter((entry) => entry.folderId === folderId);
  }

  async function handleOpenClick() {
    const selected = await pickRepositoryFolder();
    if (selected !== null) {
      onOpenPath(selected);
    }
  }

  function handleRemove(event: MouseEvent, path: string) {
    event.stopPropagation();
    onRemoveRecent(path);
  }

  async function handleNewFolder() {
    const name = await promptAsync("New folder name:", "");
    if (name) onCreateFolder(name);
  }

  async function handleNewFolderFromRepo(path: string) {
    const name = await promptAsync("New folder name:", "");
    if (!name) return;
    const folder = onCreateFolder(name);
    onMoveRepo(path, folder.id, null);
  }

  async function handleRenameFolder(folder: RepoFolder) {
    const name = await promptAsync("Rename folder:", folder.name);
    if (name) onRenameFolder(folder.id, name);
  }

  async function handleDeleteFolder(folder: RepoFolder) {
    const count = reposInFolder(folder.id).length;
    const message =
      count > 0
        ? `Delete "${folder.name}"? ${count} repo${count === 1 ? "" : "s"} will move back to Recent.`
        : `Delete "${folder.name}"?`;
    if (await confirmAsync(message)) onDeleteFolder(folder.id);
  }

  function handleRepoContextMenu(event: MouseEvent, entry: RecentRepo) {
    event.preventDefault();
    const items: ContextMenuItem[] = [];
    for (const folder of folders) {
      if (folder.id === entry.folderId) continue;
      items.push({
        label: `Move to "${folder.name}"`,
        onSelect: () => onMoveRepo(entry.path, folder.id, null),
      });
    }
    items.push({ label: "New Folder…", onSelect: () => void handleNewFolderFromRepo(entry.path) });
    if (entry.folderId) {
      items.push({ separator: true });
      items.push({
        label: "Remove from Folder",
        onSelect: () => onMoveRepo(entry.path, undefined, null),
      });
    }
    items.push({ separator: true });
    items.push({
      label: "Remove from Recent",
      danger: true,
      onSelect: () => onRemoveRecent(entry.path),
    });
    openContextMenu(event.clientX, event.clientY, items);
  }

  function handleFolderContextMenu(event: MouseEvent, folder: RepoFolder) {
    event.preventDefault();
    openContextMenu(event.clientX, event.clientY, [
      { label: "Rename Folder…", onSelect: () => void handleRenameFolder(folder) },
      { separator: true },
      { label: "Delete Folder", danger: true, onSelect: () => void handleDeleteFolder(folder) },
    ]);
  }

  // Pointer-based drag-and-drop rather than native HTML5 `draggable`/`dragover`/`drop` — the
  // latter doesn't reliably fire `dragover`/`drop` in the Tauri WebKitGTK webview (same reason
  // `CommitGraph.svelte`'s ref-drag gesture uses pointer events + `elementFromPoint` hit-testing
  // instead of native DnD). Rows/headers carry `data-*` attributes read back here rather than
  // relying on event targets, since pointer capture retargets events to the container.
  const DRAG_THRESHOLD_PX = 6;

  type DragSource = { type: "repo"; path: string } | { type: "folder"; id: string };
  type DropTarget =
    | { kind: "repo"; path: string; folderId: string | undefined }
    | { kind: "folder"; id: string }
    | { kind: "ungrouped" };

  function dropTargetKey(target: DropTarget | null): string {
    if (!target) return "";
    if (target.kind === "repo") return `repo:${target.path}`;
    if (target.kind === "folder") return `folder:${target.id}`;
    return "ungrouped";
  }

  let pointerDownInfo: { x: number; y: number; source: DragSource } | null = null;
  let dragSource = $state<DragSource | null>(null);
  let dropTarget = $state<DropTarget | null>(null);
  let suppressClick = false;

  function resolveDropTarget(clientX: number, clientY: number): DropTarget | null {
    const hit = document.elementFromPoint(clientX, clientY);
    if (!hit) return null;
    const repoEl = hit.closest<HTMLElement>("[data-repo-path]");
    if (repoEl?.dataset.repoPath) {
      return {
        kind: "repo",
        path: repoEl.dataset.repoPath,
        folderId: repoEl.dataset.folderId || undefined,
      };
    }
    const folderEl = hit.closest<HTMLElement>("[data-folder-id]");
    if (folderEl?.dataset.folderId) {
      return { kind: "folder", id: folderEl.dataset.folderId };
    }
    if (hit.closest("[data-drop-zone='ungrouped']")) {
      return { kind: "ungrouped" };
    }
    return null;
  }

  function handlePointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    const repoEl = target.closest<HTMLElement>("[data-repo-path]");
    const folderEl = !repoEl ? target.closest<HTMLElement>("[data-folder-id]") : null;
    let source: DragSource | null = null;
    if (repoEl?.dataset.repoPath) {
      source = { type: "repo", path: repoEl.dataset.repoPath };
    } else if (folderEl?.dataset.folderId) {
      source = { type: "folder", id: folderEl.dataset.folderId };
    }
    if (!source) return;
    // Every row is both a click target and a drag source here (unlike `CommitGraph.svelte`,
    // where only ref badges are drag-eligible and plain row clicks never touch pointer
    // capture at all) — so capture is deferred to `handlePointerMove`, once a real drag is
    // confirmed, rather than acquired on every single click.
    pointerDownInfo = { x: event.clientX, y: event.clientY, source };
  }

  function handlePointerMove(event: PointerEvent) {
    if (!pointerDownInfo) return;
    if (!dragSource) {
      const distance = Math.hypot(
        event.clientX - pointerDownInfo.x,
        event.clientY - pointerDownInfo.y,
      );
      if (distance < DRAG_THRESHOLD_PX) return;
      dragSource = pointerDownInfo.source;
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    }
    const candidate = resolveDropTarget(event.clientX, event.clientY);
    if (dropTargetKey(candidate) !== dropTargetKey(dropTarget)) dropTarget = candidate;
  }

  function handlePointerUp(event: PointerEvent) {
    // Only swallow the trailing `click` if a drag actually moved something — plain clicks
    // (even ones that technically crossed DRAG_THRESHOLD_PX from a bit of mouse jitter, or a
    // drag released back over its own origin) must still select/toggle/remove normally.
    if (dragSource) {
      (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
      if (dropTarget && applyDrop(dragSource, dropTarget)) suppressClick = true;
    }
    pointerDownInfo = null;
    dragSource = null;
    dropTarget = null;
  }

  function applyDrop(source: DragSource, target: DropTarget): boolean {
    if (source.type === "repo") {
      if (target.kind === "repo") {
        if (target.path === source.path) return false;
        onMoveRepo(source.path, target.folderId, target.path);
      } else if (target.kind === "folder") {
        onMoveRepo(source.path, target.id, null);
      } else {
        onMoveRepo(source.path, undefined, null);
      }
    } else if (target.kind === "folder") {
      if (target.id === source.id) return false;
      onMoveFolder(source.id, target.id);
    } else if (target.kind === "ungrouped") {
      onMoveFolder(source.id, null);
    }
    return true;
  }

  // A completed drag that actually moved something still ends in a native `click` on release;
  // swallow just that one so it doesn't also select/toggle/remove whatever's under the pointer.
  function handleClickCapture(event: MouseEvent) {
    if (!suppressClick) return;
    suppressClick = false;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

{#snippet repoRow(entry: RecentRepo)}
  <li
    class:current={entry.path === currentPath}
    class:drag-over={dropTarget?.kind === "repo" && dropTarget.path === entry.path}
    class:dragged={dragSource?.type === "repo" && dragSource.path === entry.path}
    data-repo-path={entry.path}
    data-folder-id={entry.folderId ?? ""}
    oncontextmenu={(event) => handleRepoContextMenu(event, entry)}
  >
    <button
      type="button"
      class="recent-row"
      title={entry.path}
      onclick={() => onSelectRecent(entry.path)}
    >
      <Icon name="folder" size={13} />
      <span class="recent-name">{repoDisplayName(entry.path)}</span>
    </button>
    <button
      type="button"
      class="remove"
      title={`Remove ${entry.path} from recent`}
      onclick={(event) => handleRemove(event, entry.path)}
    >
      <Icon name="x" size={12} />
    </button>
  </li>
{/snippet}

<div class="repo-rail">
  <div class="brand">
    <span class="brand-mark">
      <Logo size={20} />
    </span>
    <span class="brand-name">Push<span class="brand-name-accent">Git</span></span>
    {#if appVersion}
      <span class="brand-version">v{appVersion}</span>
    {/if}
  </div>

  {#if updateCheckState.available}
    <div class="update-banner">
      <button type="button" class="update-link" onclick={handleViewRelease}>
        <Icon name="download" size={13} />
        <span>v{updateCheckState.available.version} available</span>
      </button>
      <button
        type="button"
        class="update-dismiss"
        title="Dismiss"
        onclick={() => void dismissUpdateBanner()}
      >
        <Icon name="x" size={11} />
      </button>
    </div>
  {/if}

  <Button variant="tonal" title="Open a repository" onclick={handleOpenClick}>
    <Icon name="folder" size={14} /> Open
  </Button>

  {#if openError}
    <p class="error" role="alert">{openError}</p>
  {/if}

  <div
    class="repos"
    role="group"
    aria-label="Repositories"
    class:dragging={dragSource !== null}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onclickcapture={handleClickCapture}
  >
    {#if folders.length === 0 && recentRepos.length === 0}
      <p class="placeholder">Repos you open will show up here.</p>
    {:else}
      <div class="recent">
        <div
          class="recent-header"
          role="group"
          aria-label="Ungrouped repositories"
          data-drop-zone="ungrouped"
          class:drag-over={dropTarget?.kind === "ungrouped"}
        >
          <h3><Icon name="clock" size={12} /> Recent</h3>
        </div>
        {#if ungroupedRepos.length === 0 && folders.length === 0}
          <p class="placeholder">Repos you open will show up here.</p>
        {:else if ungroupedRepos.length > 0}
          <ul class="recent-list">
            {#each ungroupedRepos as entry (entry.path)}
              {@render repoRow(entry)}
            {/each}
          </ul>
        {/if}
      </div>

      {#each folders as folder (folder.id)}
        <div class="folder">
          <div
            class="folder-header"
            class:drag-over={dropTarget?.kind === "folder" && dropTarget.id === folder.id}
            class:dragged={dragSource?.type === "folder" && dragSource.id === folder.id}
            data-folder-id={folder.id}
            role="button"
            tabindex="0"
            oncontextmenu={(event) => handleFolderContextMenu(event, folder)}
            onclick={() => onToggleFolderCollapsed(folder.id)}
            onkeydown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onToggleFolderCollapsed(folder.id);
              }
            }}
          >
            <span class="chevron" class:collapsed={folder.collapsed}>
              <Icon name="chevron-down" size={11} />
            </span>
            <Icon name="folder" size={13} />
            <span class="folder-name">{folder.name}</span>
            <span class="folder-count">{reposInFolder(folder.id).length}</span>
          </div>
          {#if !folder.collapsed}
            <ul class="recent-list folder-repos">
              {#each reposInFolder(folder.id) as entry (entry.path)}
                {@render repoRow(entry)}
              {/each}
            </ul>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <button
    type="button"
    class="new-folder-button"
    title="Create a new folder"
    onclick={handleNewFolder}
  >
    <Icon name="plus" size={12} /> New Folder
  </button>

  <div class="sidebar-footer">
    <Popover placement="top" portal class="notifications-trigger">
      {#snippet trigger()}
        <Icon name="bell" size={13} />
        <span>Notifications</span>
        {#if unreadNotificationCount() > 0}
          <span class="count-badge">{unreadNotificationCount()}</span>
        {/if}
      {/snippet}
      {#snippet children()}
        <NotificationsPanel />
      {/snippet}
    </Popover>
    <button type="button" class="settings-button" onclick={onOpenSettings}>
      <Icon name="settings" size={13} />
      <span>Settings</span>
    </button>
  </div>
</div>

<style>
  .repo-rail {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    color: var(--text-primary);
  }

  .brand-mark {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    color: var(--on-primary-container);
    background: var(--primary-container);
    border-radius: 24%;
  }

  .brand-name {
    font-size: 1.05rem;
    font-weight: 500;
    letter-spacing: -0.01em;
  }

  .brand-name-accent {
    color: var(--accent);
  }

  .brand-version {
    margin-left: auto;
    font-size: 0.7rem;
    color: var(--text-secondary);
  }

  .new-folder-button {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.3rem;
    font: inherit;
    font-size: 0.76rem;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    align-self: flex-start;
    border-radius: var(--radius-sm);
  }

  .new-folder-button:hover {
    color: var(--accent);
  }

  .sidebar-footer {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-top: auto;
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  :global(.notifications-trigger) {
    display: flex;
    width: 100%;
  }

  :global(.notifications-trigger .trigger) {
    width: 100%;
  }

  .settings-button {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    padding: 0.35rem 0.6rem;
    font: inherit;
    font-size: 0.8rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition:
      background-color 0.1s ease,
      border-color 0.1s ease;
  }

  .settings-button:hover {
    background: var(--surface-2);
  }

  :global(.notifications-trigger) .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 1.1rem;
    height: 1.1rem;
    margin-left: auto;
    padding: 0 0.25rem;
    font-size: 0.68rem;
    font-weight: 600;
    line-height: 1;
    color: var(--accent);
    background: var(--accent-bg);
    border-radius: 999px;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: 0.8rem;
  }

  .update-banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0.35rem 0.5rem;
    background: var(--accent-bg);
    border-radius: var(--radius-md);
  }

  .update-link {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
    padding: 0;
    color: var(--accent);
    background: none;
    border: none;
    font: inherit;
    font-size: 0.78rem;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .update-dismiss {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    width: 1.3rem;
    height: 1.3rem;
    color: var(--text-muted);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .update-dismiss:hover {
    color: var(--accent);
  }

  .repos {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
    overflow: auto;
  }

  .repos.dragging {
    cursor: grabbing;
    user-select: none;
    /* WebKitGTK (Tauri on Linux) is the one that actually needs this prefixed form. */
    -webkit-user-select: none;
  }

  .folder-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0.3rem 0.5rem;
    color: var(--text-secondary);
    font-size: 0.8rem;
    font-weight: 600;
    border-radius: var(--radius-md);
    cursor: grab;
  }

  .folder-header.dragged {
    opacity: 0.5;
  }

  .folder-header:hover,
  .folder-header.drag-over {
    background: var(--surface-2);
  }

  .folder-header:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .chevron {
    display: flex;
    flex-shrink: 0;
    color: var(--text-muted);
    transition: transform 0.1s ease;
  }

  .chevron.collapsed {
    transform: rotate(-90deg);
  }

  .folder-name {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-count {
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 0.72rem;
    font-weight: 400;
  }

  .folder-repos {
    padding-left: 1.1rem;
  }

  .recent {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-height: 0;
  }

  .recent-header {
    border-radius: var(--radius-md);
  }

  .recent-header.drag-over {
    background: var(--surface-2);
  }

  .recent h3 {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin: 0;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .placeholder {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.8rem;
  }

  .recent-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .recent-list li {
    display: flex;
    align-items: center;
    border-radius: var(--radius-md);
    cursor: grab;
  }

  .recent-list li.current {
    background: var(--accent-bg);
  }

  .recent-list li.drag-over {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
  }

  .recent-list li.dragged {
    opacity: 0.5;
  }

  .recent-row {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding: 0.4rem 0.5rem;
    color: var(--text-secondary);
    background: none;
    border: none;
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-md);
  }

  li.current .recent-row {
    color: var(--accent);
    font-weight: 600;
  }

  .recent-row:hover {
    background: var(--surface-2);
  }

  li.current .recent-row:hover {
    background: var(--accent-bg);
  }

  .recent-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .remove {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.6rem;
    height: 1.6rem;
    margin-right: 0.2rem;
    color: var(--text-muted);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.1s ease;
  }

  .recent-list li:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    color: var(--danger);
    background: var(--danger-bg);
  }
</style>
