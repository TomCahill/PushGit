<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Ctrl+P / Cmd+P command palette: fuzzy (substring) search across quick commands, local
  // branches, currently-changed files, and commits. Mounted once
  // at the shell root, same imperative-singleton pattern as `ConfirmDialog`/`ContextMenu` —
  // this component owns the actual searchable data (it needs `repoPath` to fetch it), while
  // `commandPalette.svelte.ts` only tracks open/closed.
  //
  // Simplifications, each a deliberate MVP scope call rather than an oversight:
  // - Selecting a **file** switches to the working-directory view rather than also
  //   selecting that exact file's row within it — `StagingPanel` doesn't expose an external
  //   "select this path" prop today, and adding one is more scope than a palette needs.
  // - Selecting a **commit** copies its short SHA rather than jumping the graph to it — the
  //   graph has no cross-component "scroll to and select this oid" hook yet either. Copying
  //   the SHA is still immediately useful (paste it into `git show`, a PR description, etc.)
  //   and needs no new plumbing.
  // - Commit search reuses the existing `graph_open`/`graph_page`/`graph_close` session
  //   flow (the same one `CommitGraph`'s own search box drives) rather than a dedicated
  //   command — opening a short-lived, immediately-closed session per query avoids adding
  //   any new backend surface for this feature at all.
  import { fade, fly } from "svelte/transition";
  import {
    checkoutBranch,
    diffStaged,
    diffUnstaged,
    graphClose,
    graphOpen,
    graphPage,
    listBranches,
  } from "$lib/git/api";
  import type { BranchInfo, CommitRow } from "$lib/git/types";
  import {
    closeCommandPalette,
    openCommandPalette,
    paletteState,
    type PaletteCommand,
  } from "./commandPalette.svelte";
  import { motionBase } from "$lib/shell/motion";

  const SEARCH_DEBOUNCE_MS = 200;
  const MAX_PER_SECTION = 6;

  let {
    repoPath,
    commands,
    onChanged,
    onSelectFile,
  }: {
    repoPath: string;
    commands: PaletteCommand[];
    onChanged?: () => void;
    onSelectFile?: () => void;
  } = $props();

  let query = $state("");
  let activeIndex = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let error = $state<string | null>(null);

  let branches = $state<BranchInfo[]>([]);
  let filePaths = $state<string[]>([]);
  let commitResults = $state<CommitRow[]>([]);

  type Entry = {
    section: "Commands" | "Branches" | "Files" | "Commits";
    key: string;
    label: string;
    run: () => void | Promise<void>;
  };

  function matches(label: string, q: string): boolean {
    return label.toLowerCase().includes(q.toLowerCase());
  }

  const filteredCommands = $derived<Entry[]>(
    (query.trim() ? commands.filter((c) => matches(c.label, query)) : commands)
      .slice(0, MAX_PER_SECTION)
      .map((c) => ({
        section: "Commands",
        key: `cmd:${c.id}`,
        label: c.label,
        run: async () => {
          await c.run();
          closeCommandPalette();
        },
      })),
  );

  const filteredBranches = $derived<Entry[]>(
    (query.trim() ? branches.filter((b) => matches(b.name, query)) : branches)
      .slice(0, MAX_PER_SECTION)
      .map((b) => ({
        section: "Branches",
        key: `branch:${b.name}`,
        label: b.isHead ? `${b.name} (current)` : b.name,
        run: () => void handleCheckout(b),
      })),
  );

  const filteredFiles = $derived<Entry[]>(
    (query.trim() ? filePaths.filter((p) => matches(p, query)) : filePaths)
      .slice(0, MAX_PER_SECTION)
      .map((path) => ({
        section: "Files",
        key: `file:${path}`,
        label: path,
        run: () => {
          onSelectFile?.();
          closeCommandPalette();
        },
      })),
  );

  const filteredCommits = $derived<Entry[]>(
    commitResults.slice(0, MAX_PER_SECTION).map((row) => ({
      section: "Commits",
      key: `commit:${row.oid}`,
      label: `${row.shortOid} ${row.summary}`,
      run: () => void handleCopyCommit(row),
    })),
  );

  const entries = $derived<Entry[]>([
    ...filteredCommands,
    ...filteredBranches,
    ...filteredFiles,
    ...filteredCommits,
  ]);

  $effect(() => {
    void entries;
    activeIndex = 0;
  });

  $effect(() => {
    if (!paletteState.open) return;
    query = "";
    error = null;
    commitResults = [];
    queueMicrotask(() => inputEl?.focus());
    void loadStaticData(repoPath);
  });

  async function loadStaticData(path: string) {
    if (!path) {
      branches = [];
      filePaths = [];
      return;
    }
    try {
      const [branchList, unstaged, staged] = await Promise.all([
        listBranches(path),
        diffUnstaged(path),
        diffStaged(path),
      ]);
      branches = branchList;
      const paths = new Set<string>();
      for (const file of [...unstaged, ...staged]) {
        const filePath = file.newPath ?? file.oldPath;
        if (filePath) paths.add(filePath);
      }
      filePaths = [...paths].sort();
    } catch (err) {
      error = String(err);
    }
  }

  let searchGeneration = 0;
  $effect(() => {
    const q = query;
    if (!paletteState.open || !repoPath || !q.trim()) {
      commitResults = [];
      return;
    }
    const myGeneration = ++searchGeneration;
    const timer = setTimeout(() => void searchCommits(q, myGeneration), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  async function searchCommits(q: string, generation: number) {
    let sessionId: string | null = null;
    try {
      sessionId = await graphOpen(repoPath, { refs: [], search: q });
      const page = await graphPage(sessionId, MAX_PER_SECTION);
      if (generation === searchGeneration) commitResults = page.rows;
    } catch (err) {
      if (generation === searchGeneration) error = String(err);
    } finally {
      if (sessionId) void graphClose(sessionId);
    }
  }

  async function handleCheckout(branch: BranchInfo) {
    if (branch.isHead) {
      closeCommandPalette();
      return;
    }
    try {
      await checkoutBranch(repoPath, branch.name);
      onChanged?.();
      closeCommandPalette();
    } catch (err) {
      error = String(err);
    }
  }

  async function handleCopyCommit(row: CommitRow) {
    try {
      await navigator.clipboard.writeText(row.oid);
      closeCommandPalette();
    } catch (err) {
      error = String(err);
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "p") {
      event.preventDefault();
      if (paletteState.open) {
        closeCommandPalette();
      } else {
        openCommandPalette();
      }
    }
  }

  function handleDialogKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeCommandPalette();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      if (entries.length > 0) activeIndex = (activeIndex + 1) % entries.length;
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (entries.length > 0) activeIndex = (activeIndex - 1 + entries.length) % entries.length;
    } else if (event.key === "Enter") {
      event.preventDefault();
      const entry = entries[activeIndex];
      if (entry) void runEntry(entry);
    }
  }

  /** Every entry's `run()` already closes the palette on its own success path — this only
   *  needs to catch a rejection and surface it, keeping the palette open so the user sees
   *  it (matching the branch/commit entries' own inline try/catch). */
  async function runEntry(entry: Entry) {
    try {
      await entry.run();
    } catch (err) {
      error = String(err);
    }
  }

  function handleScrimClick() {
    closeCommandPalette();
  }

  // Precomputes each entry's "is this the first row of a new section" flag once, rather
  // than mutating a closure variable while `{#each}` renders — keeps section-header
  // placement a pure function of `entries` instead of a render-order side effect.
  const rows = $derived.by(() => {
    let last: string | null = null;
    return entries.map((entry) => {
      const showSectionLabel = entry.section !== last;
      last = entry.section;
      return { entry, showSectionLabel };
    });
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if paletteState.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scrim" onclick={handleScrimClick} transition:fade={{ duration: motionBase() }}>
    <div
      class="palette"
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      tabindex="-1"
      onclick={(event) => event.stopPropagation()}
      onkeydown={handleDialogKeydown}
      transition:fly={{ duration: motionBase(), y: -8 }}
    >
      <input
        bind:this={inputEl}
        type="text"
        bind:value={query}
        placeholder="Search commands, branches, files, or commits…"
        aria-label="Command palette search"
      />
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}
      {#if rows.length === 0}
        <p class="empty">No matches.</p>
      {:else}
        <ul role="listbox">
          {#each rows as { entry, showSectionLabel }, i (entry.key)}
            {#if showSectionLabel}
              <li class="section-label" role="presentation">{entry.section}</li>
            {/if}
            <li>
              <button
                type="button"
                role="option"
                aria-selected={i === activeIndex}
                class="entry"
                class:active={i === activeIndex}
                onmouseenter={() => (activeIndex = i)}
                onclick={() => void runEntry(entry)}
              >
                {entry.label}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    background: rgba(0, 0, 0, 0.35);
  }

  .palette {
    display: flex;
    flex-direction: column;
    width: min(32rem, 90vw);
    max-height: 60vh;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }

  input {
    flex-shrink: 0;
    padding: 0.75rem 1rem;
    font: inherit;
    font-size: 0.95rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: none;
    border-bottom: 1px solid var(--border);
  }

  input:focus-visible {
    outline: none;
  }

  ul {
    margin: 0;
    padding: 0.3rem;
    list-style: none;
    overflow-y: auto;
  }

  .section-label {
    padding: 0.35rem 0.6rem 0.15rem;
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
  }

  .entry {
    display: block;
    width: 100%;
    padding: 0.4rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .entry.active {
    background: var(--accent-bg);
    color: var(--accent);
  }

  .empty {
    margin: 0;
    padding: 1rem;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .error {
    margin: 0;
    padding: 0.5rem 1rem 0;
    color: var(--danger);
    font-size: 0.8rem;
  }
</style>
