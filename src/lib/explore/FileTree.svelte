<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  import { tick, untrack } from "svelte";
  import { listTree } from "$lib/git/api";
  import type { TreeEntry } from "$lib/git/types";
  import Icon from "$lib/shell/Icon.svelte";
  import FileStatusIcon from "$lib/diff/FileStatusIcon.svelte";
  import { openCommandPalette } from "$lib/palette/commandPalette.svelte";
  import { exploreState, openFile, shortOid, sourceKey } from "./explore.svelte";

  let { repoPath, refreshKey }: { repoPath: string; refreshKey: number } = $props();

  type Listing = TreeEntry[] | "loading" | { error: string };
  type Row =
    | { kind: "entry"; entry: TreeEntry; level: number }
    | { kind: "loading" | "error"; level: number; message: string; key: string };

  let listings = $state<Record<string, Listing>>({});
  let expanded = $state<Record<string, boolean>>({});
  let focusIndex = $state(0);
  let treeEl: HTMLDivElement | undefined = $state();
  let generation = 0;

  async function load(dir: string, gen: number, quiet = false) {
    if (!quiet) listings[dir] = "loading";
    try {
      const entries = await listTree(repoPath, exploreState.source, dir);
      if (gen === generation) listings[dir] = entries;
    } catch (err) {
      if (gen === generation) listings[dir] = { error: String(err) };
    }
  }

  function ancestors(path: string): string[] {
    const parts = path.split("/");
    return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
  }

  $effect(() => {
    void sourceKey(exploreState.source);
    void repoPath;
    untrack(() => {
      const gen = ++generation;
      listings = {};
      expanded = {};
      focusIndex = 0;
      void load("", gen);
    });
  });

  $effect(() => {
    const target = exploreState.reveal;
    if (!target) return;
    const gen = untrack(() => generation);
    const dirs = ancestors(target);
    void untrack(async () => {
      for (const dir of dirs) {
        expanded[dir] = true;
        if (!listings[dir]) await load(dir, gen);
      }
      exploreState.reveal = null;
      await tick();
      treeEl
        ?.querySelector(`[data-path="${CSS.escape(target)}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  });

  let lastRefresh = untrack(() => refreshKey);
  $effect(() => {
    if (refreshKey === lastRefresh) return;
    lastRefresh = refreshKey;
    untrack(() => {
      if (exploreState.source.kind !== "workingTree") return;
      for (const dir of Object.keys(listings)) void load(dir, generation, true);
    });
  });

  const rows = $derived.by(() => {
    const out: Row[] = [];
    const walk = (dir: string, level: number) => {
      const listing = listings[dir];
      if (listing === undefined) return;
      if (listing === "loading") {
        out.push({ kind: "loading", level, message: "Loading…", key: `loading:${dir}` });
      } else if ("error" in listing) {
        out.push({
          kind: "error",
          level,
          message: `Couldn't read this folder: ${listing.error}`,
          key: `error:${dir}`,
        });
      } else {
        for (const entry of listing) {
          out.push({ kind: "entry", entry, level });
          if (entry.kind === "dir" && expanded[entry.path]) walk(entry.path, level + 1);
        }
      }
    };
    walk("", 0);
    return out;
  });

  function toggle(entry: TreeEntry, open = !expanded[entry.path]) {
    expanded[entry.path] = open;
    if (open && !listings[entry.path]) void load(entry.path, generation);
  }

  function activate(entry: TreeEntry) {
    if (entry.kind === "dir") toggle(entry);
    else openFile(entry.path, entry.status);
  }

  async function moveFocus(index: number) {
    focusIndex = Math.max(0, Math.min(rows.length - 1, index));
    await tick();
    treeEl?.querySelector<HTMLElement>(`[data-index="${focusIndex}"]`)?.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    const row = rows[focusIndex];
    const entry = row?.kind === "entry" ? row.entry : null;
    switch (event.key) {
      case "ArrowDown":
        void moveFocus(focusIndex + 1);
        break;
      case "ArrowUp":
        void moveFocus(focusIndex - 1);
        break;
      case "Home":
        void moveFocus(0);
        break;
      case "End":
        void moveFocus(rows.length - 1);
        break;
      case "ArrowRight":
        if (entry?.kind === "dir" && !expanded[entry.path]) toggle(entry, true);
        else void moveFocus(focusIndex + 1);
        break;
      case "ArrowLeft":
        if (entry?.kind === "dir" && expanded[entry.path]) toggle(entry, false);
        else if (row) {
          const parentIndex = rows.findLastIndex(
            (r, i) => i < focusIndex && r.kind === "entry" && r.level === row.level - 1,
          );
          if (parentIndex >= 0) void moveFocus(parentIndex);
        }
        break;
      case "Enter":
      case " ":
        if (entry) activate(entry);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  function collapseAll() {
    expanded = {};
    focusIndex = 0;
  }
</script>

<div class="file-tree-panel">
  <div class="panel-header">
    <h2>Files</h2>
    {#if exploreState.source.kind === "revision"}
      <span class="chip">at {shortOid(exploreState.source.oid)}</span>
    {/if}
    <span class="spacer"></span>
    <button
      type="button"
      class="icon-button"
      aria-label="Collapse all folders"
      onclick={collapseAll}
    >
      <Icon name="fold" size={14} />
    </button>
  </div>
  <button type="button" class="goto" onclick={() => openCommandPalette({ filesOnly: true })}>
    <Icon name="search" size={14} />
    <span>Go to file</span>
    <span class="spacer"></span>
    <kbd>Ctrl P</kbd>
  </button>

  <div
    class="tree"
    role="tree"
    tabindex="-1"
    aria-label={exploreState.source.kind === "workingTree" ? "Working tree" : "Files"}
    bind:this={treeEl}
    onkeydown={handleKeydown}
  >
    {#each rows as row, index (row.kind === "entry" ? row.entry.path : row.key)}
      {#if row.kind === "entry"}
        {@const entry = row.entry}
        <!-- Keys are handled once on the tree (roving focus), not per row. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="row"
          class:selected={entry.kind !== "dir" && entry.path === exploreState.path}
          class:gone={entry.status === "deleted"}
          role="treeitem"
          aria-level={row.level + 1}
          aria-expanded={entry.kind === "dir" ? !!expanded[entry.path] : undefined}
          aria-selected={entry.kind !== "dir" && entry.path === exploreState.path}
          tabindex={index === focusIndex ? 0 : -1}
          data-index={index}
          data-path={entry.path}
          style:padding-left="{8 + row.level * 16}px"
          onclick={() => {
            focusIndex = index;
            activate(entry);
          }}
        >
          {#if entry.kind === "dir"}
            <span class="chev"
              ><Icon
                name={expanded[entry.path] ? "chevron-down" : "chevron-right"}
                size={14}
              /></span
            >
            <span class="folder"><Icon name="folder" size={16} /></span>
          {:else}
            <span class="chev"></span>
            <span class="file-icon"
              ><Icon name={entry.kind === "symlink" ? "link" : "file"} size={16} /></span
            >
          {/if}
          <span class="name">{entry.name}</span>
          {#if entry.containsChanges}
            <span class="dir-dot" role="img" aria-label="Contains changes"></span>
          {:else if entry.status}
            <FileStatusIcon status={entry.status} />
          {/if}
        </div>
      {:else}
        <div
          class="row message"
          class:error={row.kind === "error"}
          role={row.kind === "error" ? "alert" : "status"}
          style:padding-left="{8 + row.level * 16 + 20}px"
        >
          {row.message}
        </div>
      {/if}
    {/each}
  </div>

  <p class="footer">
    {#if exploreState.source.kind === "workingTree"}
      <Icon name="eye-off" size={14} /> Ignored files hidden
    {:else}
      <Icon name="lock" size={14} /> Pinned to {shortOid(exploreState.source.oid)}, so the content
      won't change while you read
    {/if}
  </p>
</div>

<style>
  .file-tree-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .panel-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 500;
  }

  .spacer {
    flex: 1 1 auto;
  }

  .chip {
    padding: 0.15rem 0.6rem;
    border-radius: 1rem;
    font-size: 0.78rem;
    background: var(--surface-2);
    color: var(--text-secondary);
  }

  .icon-button {
    display: inline-grid;
    place-items: center;
    width: 1.9rem;
    height: 1.9rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-1);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .goto {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
    padding: 0.45rem 0.75rem;
    font: inherit;
    font-size: 0.82rem;
    color: var(--text-secondary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    padding: 0 0.35rem;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .tree {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    font-size: 0.85rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 1.75rem;
    padding-right: 0.6rem;
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    cursor: pointer;
    user-select: none;
  }

  .row:hover {
    background: var(--surface-2);
  }

  .row.selected,
  .row.selected:hover {
    background: var(--accent-bg);
  }

  .row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .row.message {
    cursor: default;
    color: var(--text-secondary);
    font-size: 0.8rem;
  }

  .row.message:hover {
    background: none;
  }

  .row.error {
    color: var(--danger);
  }

  .chev {
    display: inline-flex;
    width: 14px;
    color: var(--text-secondary);
  }

  .folder {
    color: var(--accent);
  }

  .file-icon {
    color: var(--text-secondary);
  }

  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gone .name {
    text-decoration: line-through;
    color: var(--text-secondary);
  }

  .dir-dot {
    width: 8px;
    height: 8px;
    margin: 0 3px;
    border-radius: 50%;
    background: var(--text-secondary);
    flex-shrink: 0;
  }

  .footer {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-2) 0 0;
    padding-top: var(--space-2);
    border-top: 1px solid var(--border);
    font-size: 0.75rem;
    color: var(--text-secondary);
  }
</style>
