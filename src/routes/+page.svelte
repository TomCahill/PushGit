<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Real shell wired to the backend. Three columns: `RepoRail` (open a repo / switch
  // between recently-opened ones — purely a switcher), a center
  // column with `ActionRail` (branches/tags/stashes/remote actions, as toolbar popovers)
  // above the commit graph, and the working-directory staging UI / selected commit's diff
  // on the right.
  import "@fontsource/roboto/400.css";
  import "@fontsource/roboto/500.css";
  import "@fontsource/roboto/700.css";
  import { onDestroy, onMount } from "svelte";
  import CommitDiffView from "$lib/commit/CommitDiffView.svelte";
  import CommitGraph from "$lib/graph/CommitGraph.svelte";
  import ConflictEditor from "$lib/conflict/ConflictEditor.svelte";
  import HunkDiff from "$lib/diff/HunkDiff.svelte";
  import RepoRail from "$lib/repo/RepoRail.svelte";
  import {
    recordRepoOpened,
    removeRecentRepo,
    loadRecentRepos,
    moveRepo,
    unassignReposFromFolder,
    type RecentRepo,
  } from "$lib/repo/recentRepos";
  import {
    loadRepoFolders,
    createFolder,
    renameFolder,
    deleteFolder,
    toggleFolderCollapsed,
    moveFolder,
    type RepoFolder,
  } from "$lib/repo/repoFolders";
  import ActionRail from "$lib/shell/ActionRail.svelte";
  import Avatar from "$lib/shell/Avatar.svelte";
  import Button from "$lib/shell/Button.svelte";
  import Confetti from "$lib/shell/Confetti.svelte";
  import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
  import ContextMenu from "$lib/shell/ContextMenu.svelte";
  import ResizeHandle from "$lib/shell/ResizeHandle.svelte";
  import SettingsPanel from "$lib/settings/SettingsPanel.svelte";
  import { loadAppConfig, settingsState } from "$lib/settings/settings.svelte";
  import Toast from "$lib/shell/Toast.svelte";
  import { promptAsync } from "$lib/shell/confirmDialog.svelte";
  import {
    CENTER_MIN,
    paneWidthsState,
    setRepoRailWidth,
    setSidebarWidth,
  } from "$lib/shell/paneWidths.svelte";
  import StagingPanel from "$lib/staging/StagingPanel.svelte";
  import CommandPalette from "$lib/palette/CommandPalette.svelte";
  import { openCommandPalette } from "$lib/palette/commandPalette.svelte";
  import type { PaletteCommand } from "$lib/palette/commandPalette.svelte";
  import {
    createBranch,
    createStash,
    diffBetweenCommits,
    diffCommit,
    diffCommitToWorkdir,
    fetchRemote,
    getStartupRepoPath,
    onMenuAction,
    onRepoChanged,
    openRepository,
    pickRepositoryFolder,
    pullRemote,
    redoLastOperation,
    startRepoWatcher,
    stopRepoWatcher,
    undoLastOperation,
  } from "$lib/git/api";
  import InteractiveRebaseEditor from "$lib/rebase/InteractiveRebaseEditor.svelte";
  import CherryPickRangePicker from "$lib/cherrypick/CherryPickRangePicker.svelte";
  import BlameView from "$lib/blame/BlameView.svelte";
  import BlameFileView from "$lib/blame/BlameFileView.svelte";
  import type {
    BlameLine,
    CommitRow,
    FileDiff,
    FileDiffSelection,
    GraphFilter,
    RebaseCommitSummary,
  } from "$lib/git/types";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import AboutDialog from "$lib/shell/AboutDialog.svelte";

  const LAST_REPO_KEY = "pushgit.lastRepoPath";
  const SEARCH_DEBOUNCE_MS = 250;

  let repoPath = $state("");
  let openError = $state<string | null>(null);
  let refreshKey = $state(0);
  let recentRepos = $state<RecentRepo[]>([]);
  let folders = $state<RepoFolder[]>([]);

  let viewMode = $state<
    "working" | "commit" | "conflict" | "interactive-rebase" | "cherry-pick-range" | "blame"
  >("working");
  let rebaseOnto = $state<string | null>(null);
  let rebaseCommits = $state<RebaseCommitSummary[]>([]);
  let blamePath = $state<string | null>(null);
  let selectedCommit = $state<CommitRow | null>(null);
  let selectedFiles = $state<FileDiff[] | null>(null);
  let diffError = $state<string | null>(null);
  let conflictPath = $state<string | null>(null);
  let showSettings = $state(false);
  let showAbout = $state(false);

  // A file clicked in the right-hand sidebar's file list renders its diff in the center panel
  // (in place of the commit graph) rather than inline in the sidebar. `workingSelectedFile`/
  // `workingCenterDiff` are the working-directory case, reported up by `StagingPanel` (which
  // still owns the stage/unstage actions); `selectedCommitFilePath` is the read-only
  // selected-commit case, resolved locally against `selectedFiles` above.
  let workingSelectedFile = $state<{ path: string; staged: boolean } | null>(null);
  let workingCenterDiff = $state<FileDiffSelection | null>(null);
  let selectedCommitFilePath = $state<string | null>(null);
  let blameCenterDiff = $state<FileDiffSelection | null>(null);
  let blameLines = $state<BlameLine[] | null>(null);

  function fileKey(file: FileDiff): string {
    return file.newPath ?? file.oldPath ?? "";
  }

  const selectedCommitFile = $derived(
    selectedFiles && selectedCommitFilePath
      ? (selectedFiles.find((f) => fileKey(f) === selectedCommitFilePath) ?? null)
      : null,
  );

  const centerDiff = $derived<FileDiffSelection | null>(
    viewMode === "working"
      ? workingCenterDiff
      : viewMode === "commit" && selectedCommitFile
        ? { file: selectedCommitFile }
        : viewMode === "blame"
          ? blameCenterDiff
          : null,
  );

  // Clears whichever selection is active whenever the view mode itself changes — both
  // switching views should never show a stale file's diff, and neither selection is
  // meaningful outside "working"/"commit"/"blame" respectively.
  $effect(() => {
    void viewMode;
    workingSelectedFile = null;
    workingCenterDiff = null;
    selectedCommitFilePath = null;
    blameCenterDiff = null;
    blameLines = null;
  });

  function formatDateTime(unixSeconds: number): string {
    return new Date(unixSeconds * 1000).toLocaleString();
  }

  function closeCenterDiff() {
    workingSelectedFile = null;
    selectedCommitFilePath = null;
    blameCenterDiff = null;
  }

  type DiffMode = "parent" | "head" | "workdir";
  let diffMode = $state<DiffMode>("parent");
  let diffGeneration = 0;
  // So a workdir/HEAD comparison never looks indistinguishable from the default vs-parent view.
  const diffModeSuffix = $derived(
    diffMode === "head" ? " — vs HEAD" : diffMode === "workdir" ? " — vs working tree" : "",
  );

  // "Live as you type" means the input itself updates every keystroke,
  // but re-opening the graph session on every keystroke would be wasteful — debounce the
  // value that actually drives `graphFilter`.
  let searchQuery = $state("");
  let debouncedSearch = $state("");
  $effect(() => {
    const query = searchQuery;
    const timer = setTimeout(() => {
      debouncedSearch = query;
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });
  const graphFilter = $derived<GraphFilter>({ refs: [], search: debouncedSearch });

  let unlisten: UnlistenFn | null = null;
  let unlistenMenu: UnlistenFn | null = null;

  onMount(() => {
    recentRepos = loadRecentRepos();
    folders = loadRepoFolders();
    void loadAppConfig();
    void openInitialRepo();
    void listenForMenuActions();
    hideStartupSplash();
  });

  // Mirrors the app's "Reduce motion" setting onto the document root so this file's own
  // `:root[data-reduce-motion]` CSS override (above) can zero the `--motion-*` duration
  // tokens. The OS/DE-level `prefers-reduced-motion` case is handled independently by a plain
  // `@media` query in that same block — no JS needed for that half, so this effect only ever
  // needs to reflect the app setting, not combine the two.
  $effect(() => {
    document.documentElement.toggleAttribute("data-reduce-motion", settingsState.reduceMotion);
  });

  // `app.html`'s `#app-splash` covers the white-flash gap between window creation and this
  // component mounting (`ssr = false`) — it's
  // plain DOM driven by an inline script in `app.html` (which also owns the quote-cycling
  // timer), not a Svelte-owned element, so `window.__hideAppSplash` does the teardown.
  // Guarded for tests/environments where `app.html`'s script never ran.
  function hideStartupSplash() {
    (window as unknown as { __hideAppSplash?: () => void }).__hideAppSplash?.();
  }

  // Native File/View menu — routes
  // each item's id to the same handler its equivalent UI entry point already calls. Wrapped
  // in try/catch like `openInitialRepo` above: a context with no native menu (e.g. tests
  // that don't mock `plugin:event|listen`) just means "no menu to route from," not a startup
  // failure.
  async function listenForMenuActions() {
    try {
      unlistenMenu = await onMenuAction((id) => {
        switch (id) {
          case "open-repository":
            void handleMenuOpenRepository();
            break;
          case "settings":
            showSettings = true;
            break;
          case "command-palette":
            openCommandPalette();
            break;
          case "toggle-diff-view":
            toggleDiffView();
            break;
          case "about":
            showAbout = true;
            break;
        }
      });
    } catch {
      // No native menu in this context — nothing to listen for.
    }
  }

  // Mirrors `RepoRail.svelte`'s own `handleOpenClick` — same dialog, same `openRepo` call.
  async function handleMenuOpenRepository() {
    const selected = await pickRepositoryFolder();
    if (selected !== null) {
      void openRepo(selected);
    }
  }

  // A no-op when there's nothing to toggle to/from — no repo open, or in "working" view with
  // no commit selected yet to switch to.
  function toggleDiffView() {
    if (!repoPath) return;
    if (viewMode === "working" && selectedCommit) {
      viewMode = "commit";
    } else if (viewMode === "commit") {
      viewMode = "working";
    }
  }

  // A repo passed on the command line (`tauri-plugin-cli`) wins over the last-opened path —
  // the user explicitly asked for this one. Falls back to `localStorage` otherwise, same as
  // before that plugin existed. `getStartupRepoPath` failing (including in tests that don't
  // mock it, matching `loadAppConfig`'s established handling of the same situation) just
  // means "no CLI path" rather than blocking the fallback.
  async function openInitialRepo() {
    let cliPath: string | null = null;
    try {
      cliPath = await getStartupRepoPath();
    } catch {
      // No CLI plugin support in this context (e.g. tests) — fall back below.
    }
    const path = cliPath ?? localStorage.getItem(LAST_REPO_KEY);
    if (path) {
      void openRepo(path);
    }
  }

  onDestroy(() => {
    unlisten?.();
    unlistenMenu?.();
    void stopRepoWatcher();
  });

  async function openRepo(path: string) {
    openError = null;
    try {
      const workdir = await openRepository(path);
      repoPath = workdir;
      localStorage.setItem(LAST_REPO_KEY, workdir);
      recentRepos = recordRepoOpened(workdir);
      refreshKey += 1;

      unlisten?.();
      await startRepoWatcher(workdir);
      unlisten = await onRepoChanged(() => {
        refreshKey += 1;
      });
    } catch (err) {
      openError = String(err);
    }
  }

  function handleRemoveRecent(path: string) {
    recentRepos = removeRecentRepo(path);
  }

  function handleCreateFolder(name: string): RepoFolder {
    const result = createFolder(name);
    folders = result.folders;
    return result.folder;
  }

  function handleRenameFolder(id: string, name: string) {
    folders = renameFolder(id, name);
  }

  function handleDeleteFolder(id: string) {
    folders = deleteFolder(id);
    recentRepos = unassignReposFromFolder(id);
  }

  function handleToggleFolderCollapsed(id: string) {
    folders = toggleFolderCollapsed(id);
  }

  function handleMoveRepo(path: string, folderId: string | undefined, beforePath: string | null) {
    recentRepos = moveRepo(path, folderId, beforePath);
  }

  function handleMoveFolder(id: string, beforeId: string | null) {
    folders = moveFolder(id, beforeId);
  }

  // Generation-guarded so two independent diff loads racing (e.g. a right-click's own
  // `onSelect`-driven "vs parent" load, immediately followed by picking "Diff against HEAD"
  // from the context menu it just opened) can't have the earlier one silently overwrite the
  // later one's result if it happens to resolve second.
  async function loadDiff(commit: CommitRow, mode: DiffMode) {
    const myGeneration = ++diffGeneration;
    selectedFiles = null;
    diffError = null;
    try {
      const files =
        mode === "head"
          ? await diffBetweenCommits(repoPath, commit.oid, "HEAD")
          : mode === "workdir"
            ? await diffCommitToWorkdir(repoPath, commit.oid)
            : await diffCommit(repoPath, commit.oid);
      if (myGeneration !== diffGeneration) return;
      selectedFiles = files;
    } catch (err) {
      if (myGeneration === diffGeneration) diffError = String(err);
    }
  }

  function handleSelect(commit: CommitRow | null) {
    selectedCommit = commit;
    diffMode = "parent";
    selectedFiles = null;
    diffError = null;
    selectedCommitFilePath = null;
    if (!commit) return;
    viewMode = "commit";
    void loadDiff(commit, "parent");
  }

  function handleDiffRequest(commit: CommitRow, mode: "head" | "workdir") {
    selectedCommit = commit;
    diffMode = mode;
    viewMode = "commit";
    selectedCommitFilePath = null;
    void loadDiff(commit, mode);
  }

  function showWorkingDirectory() {
    viewMode = "working";
  }

  function handleResolveConflict(path: string) {
    conflictPath = path;
    viewMode = "conflict";
  }

  function handleInteractiveRebase(onto: string, commits: RebaseCommitSummary[]) {
    rebaseOnto = onto;
    rebaseCommits = commits;
    viewMode = "interactive-rebase";
  }

  function handleInteractiveRebaseCompleted() {
    rebaseOnto = null;
    rebaseCommits = [];
    viewMode = "working";
    refreshKey += 1;
  }

  /** Stays on the interactive-rebase view — the editor shows its own conflict banner — but
   *  still bumps `refreshKey` so `BranchSidebar` picks up the new `repository_state()`. */
  function handleInteractiveRebaseConflicts() {
    refreshKey += 1;
  }

  function showCherryPickRangePicker() {
    viewMode = "cherry-pick-range";
  }

  function handleCherryPickRangeCompleted() {
    viewMode = "working";
    refreshKey += 1;
  }

  function handleCherryPickRangeCancel() {
    viewMode = "working";
    refreshKey += 1;
  }

  /** Same reasoning as `handleInteractiveRebaseConflicts` — stay put, just refresh. */
  function handleCherryPickRangeConflicts() {
    refreshKey += 1;
  }

  function handleBlame(path: string) {
    blamePath = path;
    viewMode = "blame";
  }

  function handleBlameClose() {
    blamePath = null;
    viewMode = "working";
  }

  function handleInteractiveRebaseCancel() {
    rebaseOnto = null;
    rebaseCommits = [];
    viewMode = "working";
    refreshKey += 1;
  }

  function handleConflictResolved() {
    conflictPath = null;
    viewMode = "working";
    refreshKey += 1;
  }

  function handleConflictCancel() {
    conflictPath = null;
    viewMode = "working";
  }

  // Backing data for the Ctrl+P command palette — a handful of
  // quick actions that don't need the richer per-action UI (progress bars, busy states)
  // their "home" panels (`ActionRail`/`RemotePanel`/`UndoRedoControls`) already provide;
  // this is a fast path to the same underlying commands, not a replacement for those panels.
  const paletteCommands = $derived<PaletteCommand[]>([
    {
      id: "open-repository",
      label: "Open Repository…",
      run: async () => {
        const path = await pickRepositoryFolder();
        if (path) await openRepo(path);
      },
    },
    ...(repoPath
      ? [
          {
            id: "show-working-directory",
            label: "Show Working Directory",
            run: showWorkingDirectory,
          },
          {
            id: "create-branch",
            label: "Create Branch…",
            run: async () => {
              const name = (await promptAsync("New branch name:"))?.trim();
              if (!name) return;
              await createBranch(repoPath, name);
              refreshKey += 1;
            },
          },
          {
            id: "create-stash",
            label: "Create Stash…",
            run: async () => {
              const message = await promptAsync("Stash message (optional):", "");
              if (message === null) return;
              await createStash(repoPath, message.trim() || undefined);
              refreshKey += 1;
            },
          },
          {
            id: "fetch",
            label: "Fetch from origin",
            run: async () => {
              await fetchRemote(repoPath, "origin");
              refreshKey += 1;
            },
          },
          {
            id: "pull",
            label: "Pull from origin",
            run: async () => {
              const outcome = await pullRemote(repoPath, "origin");
              if (outcome.kind === "conflicts") showWorkingDirectory();
              refreshKey += 1;
            },
          },
          {
            id: "undo",
            label: "Undo Last Operation",
            run: async () => {
              await undoLastOperation(repoPath);
              refreshKey += 1;
            },
          },
          {
            id: "redo",
            label: "Redo Last Operation",
            run: async () => {
              await redoLastOperation(repoPath);
              refreshKey += 1;
            },
          },
          {
            id: "cherry-pick-range",
            label: "Cherry-pick Commits…",
            run: showCherryPickRangePicker,
          },
        ]
      : []),
  ]);
</script>

<main
  class="shell"
  style:grid-template-columns="{paneWidthsState.repoRail}px 5px minmax({CENTER_MIN}px, 1fr) 5px {paneWidthsState.sidebar}px"
>
  <aside class="repo-rail" aria-label="Repositories">
    <RepoRail
      currentPath={repoPath}
      {recentRepos}
      {folders}
      {openError}
      onOpenPath={openRepo}
      onSelectRecent={openRepo}
      onRemoveRecent={handleRemoveRecent}
      onCreateFolder={handleCreateFolder}
      onRenameFolder={handleRenameFolder}
      onDeleteFolder={handleDeleteFolder}
      onToggleFolderCollapsed={handleToggleFolderCollapsed}
      onMoveRepo={handleMoveRepo}
      onMoveFolder={handleMoveFolder}
      onOpenSettings={() => (showSettings = true)}
    />
  </aside>

  <ResizeHandle onResize={(dx) => setRepoRailWidth(paneWidthsState.repoRail + dx)} />

  <div class="center">
    <div class="action-rail-slot" aria-label="Branches, tags, and stashes">
      {#if repoPath}
        <ActionRail
          {repoPath}
          {refreshKey}
          onChanged={() => (refreshKey += 1)}
          onConflicts={showWorkingDirectory}
          onApplied={showWorkingDirectory}
          onSearchChange={(query) => (searchQuery = query)}
          onInteractiveRebase={handleInteractiveRebase}
        />
      {/if}
    </div>
    {#if showSettings}
      <section class="center-diff" aria-label="Settings">
        <div class="center-diff-header">
          <span class="center-diff-path">Settings</span>
          <Button variant="text" onclick={() => (showSettings = false)}>
            ← Back to {viewMode === "blame" ? "blame" : "graph"}
          </Button>
        </div>
        <div class="center-diff-body">
          <SettingsPanel {repoPath} />
        </div>
      </section>
    {:else if centerDiff}
      <section class="center-diff" aria-label="File diff">
        <div class="center-diff-header">
          <span class="center-diff-path">{fileKey(centerDiff.file)}</span>
          <Button variant="text" onclick={closeCenterDiff}>
            ← Back to {viewMode === "blame" ? "blame" : "graph"}
          </Button>
        </div>
        <div class="center-diff-body">
          <HunkDiff
            file={centerDiff.file}
            hunkActionLabel={centerDiff.hunkActionLabel}
            onHunkAction={centerDiff.onHunkAction}
            lineActionLabel={centerDiff.lineActionLabel}
            onLineAction={centerDiff.onLineAction}
          />
        </div>
      </section>
    {:else if viewMode === "blame" && blamePath}
      <section class="center-diff" aria-label="Blame">
        <div class="center-diff-header">
          <span class="center-diff-path">{blamePath}</span>
        </div>
        <div class="center-diff-body">
          {#if blameLines}
            <BlameFileView lines={blameLines} path={blamePath} />
          {:else}
            <p class="placeholder">Loading…</p>
          {/if}
        </div>
      </section>
    {:else}
      <section class="graph" aria-label="Commit graph">
        <CommitGraph
          {repoPath}
          {refreshKey}
          filter={graphFilter}
          onSelect={handleSelect}
          onChanged={() => (refreshKey += 1)}
          onConflicts={showWorkingDirectory}
          onDiffRequest={handleDiffRequest}
          onSelectWorkdir={showWorkingDirectory}
          onApplied={showWorkingDirectory}
        />
      </section>
    {/if}
  </div>

  <ResizeHandle onResize={(dx) => setSidebarWidth(paneWidthsState.sidebar - dx)} />

  <section class="diff" aria-label="Diff viewer">
    {#if !repoPath}
      <p class="placeholder">Open a repository to get started.</p>
    {:else if viewMode === "working"}
      <StagingPanel
        {repoPath}
        {refreshKey}
        bind:selectedFile={workingSelectedFile}
        onDiffChange={(diff) => (workingCenterDiff = diff)}
        onChanged={() => (refreshKey += 1)}
        onResolveConflict={handleResolveConflict}
        onBlame={handleBlame}
      />
    {:else if viewMode === "conflict"}
      {#if conflictPath}
        {#key conflictPath}
          <ConflictEditor
            {repoPath}
            path={conflictPath}
            onResolved={handleConflictResolved}
            onCancel={handleConflictCancel}
          />
        {/key}
      {/if}
    {:else if viewMode === "interactive-rebase"}
      {#if rebaseOnto}
        {#key rebaseOnto}
          <InteractiveRebaseEditor
            {repoPath}
            onto={rebaseOnto}
            commits={rebaseCommits}
            onCompleted={handleInteractiveRebaseCompleted}
            onConflicts={handleInteractiveRebaseConflicts}
            onCancel={handleInteractiveRebaseCancel}
          />
        {/key}
      {/if}
    {:else if viewMode === "cherry-pick-range"}
      <CherryPickRangePicker
        {repoPath}
        onCompleted={handleCherryPickRangeCompleted}
        onConflicts={handleCherryPickRangeConflicts}
        onCancel={handleCherryPickRangeCancel}
      />
    {:else if viewMode === "blame"}
      {#if blamePath}
        {#key blamePath}
          <BlameView
            {repoPath}
            path={blamePath}
            onClose={handleBlameClose}
            onDiffChange={(diff) => (blameCenterDiff = diff)}
            onLinesChange={(lines) => (blameLines = lines)}
          />
        {/key}
      {/if}
    {:else if !selectedCommit}
      <p class="placeholder">Select a commit to see what it changed.</p>
    {:else}
      <h2>{selectedCommit.summary}</h2>
      {#if selectedCommit.body}
        <p class="commit-body">{selectedCommit.body}</p>
      {/if}
      <p class="commit-meta">
        {selectedCommit.shortOid} - <Avatar
          name={selectedCommit.authorName}
          email={selectedCommit.authorEmail}
          size={14}
        />
        - {formatDateTime(selectedCommit.authorTime)}
        {diffModeSuffix}
      </p>
      {#if diffError}
        <p class="error" role="alert">{diffError}</p>
      {:else if selectedFiles === null}
        <p>Loading…</p>
      {:else}
        {#key selectedCommit.oid}
          <CommitDiffView files={selectedFiles} bind:selectedPath={selectedCommitFilePath} />
        {/key}
      {/if}
    {/if}
  </section>
</main>

<ConfirmDialog />
<ContextMenu />
<Toast />
<Confetti />
<AboutDialog open={showAbout} onClose={() => (showAbout = false)} />
<CommandPalette
  {repoPath}
  commands={paletteCommands}
  onChanged={() => (refreshKey += 1)}
  onSelectFile={showWorkingDirectory}
/>

<style>
  :global(:root) {
    color-scheme: dark light;

    /* Spacing scale */
    --space-1: 0.25rem;
    --space-2: 0.5rem;
    --space-3: 0.75rem;
    --space-4: 1rem;
    --space-5: 1.5rem;
    --space-6: 2rem;

    /* Radii — Material Design 3 shape scale (brand sheet: 16 small surfaces, 20 buttons,
       28 cards/icon containers). Dark theme is the default surface per the selected
       PushGit brand direction; light is kept as the alt theme via light-dark(). */
    --radius-sm: 16px;
    --radius-md: 20px;
    --radius-lg: 28px;

    /* Surfaces — M3 tonal roles (surface / surface-container / surface-container-high) */
    --surface-0: light-dark(#fef7ff, #141218);
    --surface-1: light-dark(#f3edf7, #211f26);
    --surface-2: light-dark(#ece6f0, #2b2930);
    --border: light-dark(#cac4d0, #49454f);
    --border-strong: light-dark(#79747e, #938f99);

    /* Text — on-surface / on-surface-variant roles */
    --text-primary: light-dark(#1d1b20, #e6e0e9);
    --text-secondary: light-dark(#49454f, #cac4d0);
    --text-muted: light-dark(#79747e, #938f99);

    /* Primary (brand purple seed, #6750a4 family) + secondary container, used for accents,
       links, and selection states. */
    --accent: light-dark(#6750a4, #d0bcff);
    --accent-bg: light-dark(rgba(103, 80, 164, 0.1), rgba(208, 188, 255, 0.14));
    --on-accent: light-dark(#ffffff, #381e72);
    --primary-container: light-dark(#eaddff, #4f378b);
    --on-primary-container: light-dark(#21005d, #eaddff);
    --secondary-container: light-dark(#e8def8, #4a4458);
    --on-secondary-container: light-dark(#1d192b, #e8def8);

    /* Material filled/tonal/outlined button roles (brand sheet §Buttons) */
    --btn-filled-bg: var(--accent);
    --btn-filled-fg: var(--on-accent);
    --btn-tonal-bg: var(--secondary-container);
    --btn-tonal-fg: var(--on-secondary-container);
    --btn-outlined-fg: var(--accent);
    --btn-outlined-border: var(--border-strong);

    /* Semantic colors — intentionally kept as the existing Okabe-Ito colorblind-safe hues
       (shared with the graph palette in `src/lib/graph/palette.ts` and the diff view)
       rather than the M3 baseline error role, so add/remove/conflict stay distinguishable
       together for colorblind users. */
    --success: light-dark(#00785a, #009e73);
    --success-bg: light-dark(rgba(0, 158, 115, 0.12), rgba(0, 158, 115, 0.18));
    --danger: light-dark(#a3390a, #d55e00);
    --danger-bg: light-dark(rgba(213, 94, 0, 0.12), rgba(213, 94, 0, 0.18));
    --warning: light-dark(#8a6900, #e69f00);
    --warning-bg: light-dark(rgba(230, 159, 0, 0.15), rgba(230, 159, 0, 0.2));

    /* Shadows — M3 elevation (levels 1 and 3), flat black regardless of theme */
    --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.3), 0 1px 3px 1px rgba(0, 0, 0, 0.15);
    --shadow-md: 0 1px 3px rgba(0, 0, 0, 0.3), 0 4px 8px 3px rgba(0, 0, 0, 0.15);

    --font-mono: ui-monospace, "SF Mono", "Cascadia Code", monospace;

    /* Motion — M3 standard easing. Zeroed below whenever motion should be reduced, either by
       the app's own "Reduce motion" setting or the OS/DE's `prefers-reduced-motion`, so any
       future `transition`/`animation` that reads these durations gets both for free. */
    --motion-fast: 120ms;
    --motion-base: 200ms;
    --motion-easing: cubic-bezier(0.2, 0, 0, 1);
  }

  :global(:root[data-reduce-motion]) {
    --motion-fast: 0ms;
    --motion-base: 0ms;
  }

  @media (prefers-reduced-motion: reduce) {
    :global(:root) {
      --motion-fast: 0ms;
      --motion-base: 0ms;
    }
  }

  :global(body) {
    margin: 0;
    color: var(--text-primary);
    background: var(--surface-0);
    font-family: Roboto, system-ui, sans-serif;
  }

  :global(button) {
    font-family: inherit;
  }

  /* Themed scrollbars everywhere (repo rail, graph, diff/staging, popovers, hunk lists) —
     `scrollbar-color`/`-width` cover Firefox, `::-webkit-scrollbar` covers the WebKitGTK
     webview PushGit actually ships in. */
  :global(*) {
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
  }

  :global(*::-webkit-scrollbar) {
    width: 10px;
    height: 10px;
    margin-left: 20px;
  }

  :global(*::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(*::-webkit-scrollbar-thumb) {
    background-color: var(--border-strong);
    border-style: solid;
    border-color: transparent;
    border-width: 2px 1px 2px 3px;
    border-radius: var(--radius-md);
    background-clip: padding-box;
  }

  :global(*::-webkit-scrollbar-thumb:hover) {
    background-color: var(--text-muted);
  }

  :global(*::-webkit-scrollbar-corner) {
    background: transparent;
  }

  .shell {
    display: grid;
    height: 100vh;
  }

  .repo-rail,
  .diff {
    overflow: auto;
    min-height: 0;
  }

  .repo-rail {
    padding: var(--space-4);
    background: var(--surface-1);
    border-right: 1px solid var(--border);
  }

  .center {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border-right: 1px solid var(--border);
  }

  .graph {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }

  .center-diff {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    padding: var(--space-4);
    gap: var(--space-2);
  }

  .center-diff-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .center-diff-header :global(.btn) {
    flex-shrink: 0;
  }

  .center-diff-path {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.85rem;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .center-diff-body {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
  }

  .diff {
    padding: var(--space-4);
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .placeholder {
    color: var(--text-muted);
  }

  .commit-body {
    margin-top: 0;
    max-height: 8rem;
    overflow-y: auto;
    white-space: pre-wrap;
    color: var(--text-primary);
    font-size: 0.85rem;
  }

  .commit-meta {
    margin-top: 0;
    color: var(--text-muted);
    font-size: 0.85rem;
    font-family: var(--font-mono);
  }
</style>
