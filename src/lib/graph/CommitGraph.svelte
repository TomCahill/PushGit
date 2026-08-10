<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Virtualized SVG commit graph renderer. This
  // component is a "dumb renderer": all topology/lane/color decisions already happened in
  // Rust, this only draws the `CommitRow`/`Rail` geometry it's handed and paginates via
  // `graph_page` as the user scrolls.
  //
  // Simplification vs. the original design's exact DOM sketch: each visible row owns its own small
  // `<svg>` gutter (dot + that row's rails) instead of one giant pane-wide `<svg>`. This is
  // visually identical and makes per-row virtualization (mount/unmount on scroll) trivial,
  // at the cost of not literally matching §4.1's single-`<svg>` structure.
  import {
    applyStash,
    checkoutBranch,
    checkoutCommit,
    checkoutRemoteBranch,
    cherryPickCommit,
    createBranch,
    createTag,
    deleteBranch,
    deleteTag,
    dropStash,
    fetchRemote,
    graphClose,
    graphOpen,
    graphPage,
    mergeBranch,
    moveTag,
    popStash,
    pullRemote,
    pushRemote,
    rebaseBranch,
    renameTag,
    resetTo,
  } from "$lib/git/api";
  import type {
    CherryPickOutcome,
    CommitGraphPage,
    CommitRow,
    GraphFilter,
    MergeOutcome,
    RebaseOutcome,
    Rail,
    RefMarker,
    ResetMode,
  } from "$lib/git/types";
  import { colorFor } from "./palette";
  import { decideDragAction, type DragActionResult, type DragTarget } from "./dragAction";
  import { settingsState } from "$lib/settings/settings.svelte";
  import { confirmAsync, promptAsync } from "$lib/shell/confirmDialog.svelte";
  import { notifySuccess } from "$lib/shell/toast.svelte";
  import {
    closeContextMenu,
    openContextMenu,
    type ContextMenuItem,
  } from "$lib/shell/contextMenu.svelte";
  import Avatar from "$lib/shell/Avatar.svelte";
  import CopyButton from "$lib/shell/CopyButton.svelte";

  // Menu-triggered fetch/pull/push always target "origin" — there's no `list_remotes`
  // backend command yet, matching the same narrowing `RemotePanel.svelte` already uses.
  const REMOTE_NAME = "origin";
  const REJECTED_PUSH_PATTERN = /rejected|non-fast-forward|fetch first/i;

  const ROW_HEIGHT = 24;
  const LANE_WIDTH = 16;
  const DOT_RADIUS = 4;
  const OVERSCAN = 15;
  const LOAD_THRESHOLD_PX = ROW_HEIGHT * 20;
  // Minimum pointer travel before a pointerdown on a badge becomes a drag rather than an
  // ordinary click-to-select — see `handlePointerMove`.
  const DRAG_THRESHOLD_PX = 4;
  const DEFAULT_FILTER: GraphFilter = { refs: [], search: "" };

  let {
    repoPath,
    filter = DEFAULT_FILTER,
    refreshKey = 0,
    onSelect,
    onChanged,
    onConflicts,
    onDiffRequest,
    onSelectWorkdir,
    onApplied,
  }: {
    repoPath: string;
    filter?: GraphFilter;
    /** Bump to refetch in place (new commit, branch/tag/stash change, watcher event) without
     *  touching scroll position or selection — see `reset()`'s `isSameGraph` branch. Every
     *  sibling panel (`BranchSidebar`, `StashPanel`, ...) takes this same prop instead of
     *  being force-remounted by the caller. */
    refreshKey?: number;
    onSelect?: (commit: CommitRow | null) => void;
    /** Called after a menu-triggered action (checkout/create/delete/merge/push/...) succeeds,
     *  so the parent can bump its `refreshKey` the same way every sibling panel already does. */
    onChanged?: () => void;
    /** Called when a menu-triggered merge/pull hits conflicts, so the parent can switch to the
     *  working-directory view — matches `BranchSidebar`'s/`RemotePanel`'s existing contract. */
    onConflicts?: () => void;
    /** Fired only by the commit-dot menu's "Diff against..." actions — deliberately separate
     *  from `onSelect`, which always means "vs first parent." */
    onDiffRequest?: (commit: CommitRow, mode: "head" | "workdir") => void;
    /** Fired instead of `onSelect` when the synthetic "uncommitted changes" row is clicked —
     *  it has no real oid to diff, so the parent should switch to the working-directory view
     *  instead, matching `ActionRail`'s "Working directory" toggle. */
    onSelectWorkdir?: () => void;
    /** Fired after a stash row's context-menu apply/pop succeeds — matches `StashPanel`'s own
     *  `onApplied`, so the parent switches to the working-directory view either way. */
    onApplied?: () => void;
  } = $props();

  let rows = $state<CommitRow[]>([]);
  let hasMore = $state(false);
  let loading = $state(false);
  /** Drives the visible "Loading…" line — unlike `loading` (which also gates concurrent
   *  fetches), this stays false for a same-graph background refresh so it doesn't flash
   *  below the still-displayed old rows on every watcher-triggered refresh; see `reset()`'s
   *  `isSameGraph` branch. True first loads and scroll-triggered `loadMore()` still show it. */
  let showLoading = $state(false);
  let error = $state<string | null>(null);
  let selectedOid = $state<string | null>(null);
  let maxLane = $state(0);
  let scrollTop = $state(0);
  let clientHeight = $state(0);

  let busy = $state(false);
  let actionError = $state<string | null>(null);
  let actionMessage = $state<string | null>(null);

  // Drag-to-merge/rebase state. `dragSource` is only set once the pointer has moved past
  // `DRAG_THRESHOLD_PX` from its pointerdown position — see `handlePointerMove` — so a plain
  // click on a badge (which already selects its row via bubbling to `handleClick`) is never
  // hijacked into an accidental drag.
  let dragSource = $state<{ ref: RefMarker; commitOid: string } | null>(null);
  let dropTarget = $state<DragTarget | null>(null);
  let dragPointerPos = $state<{ x: number; y: number } | null>(null);
  // Alt held while dragging the current branch onto a bare commit row picks "reset" instead of
  // the default "rebase" — see `dragAction.ts`'s header comment. Tracked live from pointermove
  // (rather than only at drop) so the tooltip reflects the modifier the instant it's pressed.
  let dragModifierHeld = $state(false);
  const dragAction = $derived<DragActionResult>(
    dragSource && dropTarget
      ? decideDragAction(dragSource, dropTarget, { modifierHeld: dragModifierHeld })
      : null,
  );

  // Plain (non-reactive) vars, mirroring `ResizeHandle.svelte`'s `last` — only read/written
  // synchronously within the pointer handlers below, never need to trigger a re-render on their
  // own.
  let pointerDownInfo: { x: number; y: number; ref: RefMarker; commitOid: string } | null = null;
  let suppressNextClick = false;

  let sessionId: string | null = null;
  let generation = 0;
  let previousPath: string | null = null;
  let previousFilterKey: string | null = null;

  $effect(() => {
    void reset(repoPath, filter, refreshKey);
  });

  $effect(() => {
    void repoPath;
    actionMessage = null;
    actionError = null;
  });

  // Unmount-only cleanup: `reset()` already closes the *previous* session each time it
  // runs, but nothing else closes the *last* one when the component itself goes away.
  $effect(() => {
    return () => {
      if (sessionId) void graphClose(sessionId);
    };
  });

  function applyPage(page: CommitGraphPage, replace: boolean) {
    rows = replace ? page.rows : [...rows, ...page.rows];
    hasMore = page.hasMore;
    if (replace) maxLane = 0;
    for (const row of page.rows) {
      const laneNumbers = [row.lane, ...row.rails.flatMap((r) => [r.fromLane, r.toLane])];
      maxLane = Math.max(maxLane, ...laneNumbers);
    }
  }

  // `refreshKey` bumps on every sibling action (stage/commit, branch/tag/stash change, a
  // watcher-detected external change) — not just on an actual new commit — so this always
  // re-opens a fresh graph session (the commit set may have changed). `isSameGraph` (same
  // `repoPath`/`filter` as last time, only `refreshKey` differs) decides *how* to apply the
  // result: for an actual repo/filter switch, clear immediately (old rows are for a
  // different subject and showing them would be misleading); for a same-subject refresh,
  // keep the previously-rendered rows on screen until the fresh first page replaces them,
  // instead of blanking the pane on every single background refresh — the visible "flash"
  // this used to cause was barely noticeable on small test repos but glaring on a large one.
  async function reset(path: string, currentFilter: GraphFilter, _refreshKey: number) {
    const myGeneration = ++generation;
    const previousSession = sessionId;
    const filterKey = JSON.stringify(currentFilter);
    const isSameGraph = path === previousPath && filterKey === previousFilterKey;
    previousPath = path;
    previousFilterKey = filterKey;

    sessionId = null;
    hasMore = false;
    error = null;
    if (!isSameGraph) {
      rows = [];
      selectedOid = null;
      maxLane = 0;
      scrollTop = 0;
      onSelect?.(null);
      closeContextMenu();
    }

    if (previousSession) void graphClose(previousSession);
    if (!path) {
      rows = [];
      return;
    }

    try {
      const id = await graphOpen(path, currentFilter);
      if (myGeneration !== generation) {
        void graphClose(id);
        return;
      }
      sessionId = id;
      hasMore = true;
      loading = true;
      if (!isSameGraph) showLoading = true;
      const page = await graphPage(id, settingsState.maxCommitsRendered);
      if (myGeneration !== generation) return;
      applyPage(page, true);
    } catch (err) {
      if (myGeneration === generation) error = String(err);
    } finally {
      if (myGeneration === generation) {
        loading = false;
        showLoading = false;
      }
    }
  }

  async function loadMore(myGeneration = generation) {
    if (loading || !hasMore || !sessionId) return;
    loading = true;
    showLoading = true;
    try {
      const page = await graphPage(sessionId, settingsState.maxCommitsRendered);
      if (myGeneration !== generation) return;
      applyPage(page, false);
    } catch (err) {
      if (myGeneration === generation) error = String(err);
    } finally {
      if (myGeneration === generation) {
        loading = false;
        showLoading = false;
      }
    }
  }

  const contentHeight = $derived(rows.length * ROW_HEIGHT);
  const gutterWidth = $derived(Math.max(1, maxLane + 1) * LANE_WIDTH);
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN));
  const endIndex = $derived(
    Math.min(rows.length, Math.ceil((scrollTop + clientHeight) / ROW_HEIGHT) + OVERSCAN),
  );
  const visibleRows = $derived(rows.slice(startIndex, endIndex));

  function laneX(lane: number): number {
    return lane * LANE_WIDTH + LANE_WIDTH / 2;
  }

  // A row's commit dot sits at the row's vertical midpoint (see `cy={ROW_HEIGHT / 2}` below),
  // not at its top/bottom edge — so a curve must be anchored at that midpoint on whichever end
  // touches this row's own commit, or it visibly crosses through the straight line between
  // dots instead of meeting them.
  function railPath(rail: Rail): string {
    const x1 = laneX(rail.fromLane);
    const x2 = laneX(rail.toLane);
    if (x1 === x2) return `M ${x1} 0 L ${x1} ${ROW_HEIGHT}`;

    const midY = ROW_HEIGHT / 2;
    if (rail.kind === "merge_edge") {
      // Originates at this row's own dot (fromLane is always this row's own lane for a merge
      // edge) and curves down to the next row's lane by the bottom edge. The segment above the
      // dot is already drawn by this row's own straight parent-edge rail.
      const ctrlY = (midY + ROW_HEIGHT) / 2;
      return `M ${x1} ${midY} C ${x1} ${ctrlY}, ${x2} ${ctrlY}, ${x2} ${ROW_HEIGHT}`;
    }
    // A converging lane — `passThrough` only has differing lanes when another lane merges into
    // this row's own commit — curves up from the row's top edge and ends exactly at this row's
    // dot (toLane is always this row's own lane here), not past it.
    const ctrlY = midY / 2;
    return `M ${x1} 0 C ${x1} ${ctrlY}, ${x2} ${ctrlY}, ${x2} ${midY}`;
  }

  let containerEl: HTMLDivElement | undefined;

  function maybeLoadMore(el: HTMLElement) {
    const distanceFromBottom = el.scrollHeight - (el.scrollTop + el.clientHeight);
    if (hasMore && !loading && distanceFromBottom < LOAD_THRESHOLD_PX) {
      void loadMore();
    }
  }

  function handleScroll(event: Event) {
    const el = event.currentTarget as HTMLDivElement;
    scrollTop = el.scrollTop;
    maybeLoadMore(el);
  }

  function select(commit: CommitRow) {
    selectedOid = commit.oid;
    // The workdir row's oid is a sentinel, not a real commit — there's nothing to diff, so
    // route to the dedicated callback instead of `onSelect`.
    if (commit.kind === "workdir") {
      onSelectWorkdir?.();
      return;
    }
    onSelect?.(commit);
  }

  function handleClick(event: MouseEvent) {
    // A completed drag (pointerdown → move past threshold → pointerup) synthesizes a trailing
    // `click` at release — without this guard, a drag-triggered merge/rebase would immediately
    // re-fire selection against whatever's now under the cursor.
    if (suppressNextClick) {
      suppressNextClick = false;
      return;
    }
    const target = (event.target as HTMLElement).closest<HTMLElement>("[data-oid]");
    if (!target?.dataset.oid) return;
    const commit = rows.find((r) => r.oid === target.dataset.oid);
    if (commit) select(commit);
  }

  async function runAction(fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    actionError = null;
    try {
      await fn();
      onChanged?.();
    } catch (err) {
      actionError = String(err);
    } finally {
      busy = false;
    }
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard access can be denied — a silent no-op beats an error banner for a purely
      // cosmetic convenience action, matching `CopyButton.svelte`'s own handling.
    }
  }

  function describeMergeOutcome(branchName: string, outcome: MergeOutcome): string {
    switch (outcome.kind) {
      case "fast_forward":
        return `Fast-forwarded to ${branchName}.`;
      case "already_up_to_date":
        return "Already up to date.";
      case "merged":
        return `Merged ${branchName}.`;
      case "conflicts":
        return `Merge stopped with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function describePullOutcome(outcome: MergeOutcome): string {
    switch (outcome.kind) {
      case "fast_forward":
        return "Pulled — fast-forwarded.";
      case "already_up_to_date":
        return "Already up to date.";
      case "merged":
        return "Pulled and merged.";
      case "conflicts":
        return `Pull stopped with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function handleCheckoutCommit(commit: CommitRow) {
    void runAction(() => checkoutCommit(repoPath, commit.oid));
  }

  function handleCreateBranchHere(commit: CommitRow) {
    void (async () => {
      const name = (await promptAsync("Branch name:"))?.trim();
      if (!name) return;
      await runAction(() => createBranch(repoPath, name, commit.oid));
    })();
  }

  function handleCreateTagHere(commit: CommitRow) {
    void (async () => {
      const name = (await promptAsync("Tag name:"))?.trim();
      if (!name) return;
      await runAction(() => createTag(repoPath, name, commit.oid));
    })();
  }

  function handleDeleteTag(name: string) {
    void (async () => {
      if (!(await confirmAsync(`Delete tag "${name}"?`))) return;
      await runAction(() => deleteTag(repoPath, name));
    })();
  }

  function handleRenameTag(name: string) {
    void (async () => {
      const newName = (await promptAsync("New tag name:", name))?.trim();
      if (!newName || newName === name) return;
      await runAction(() => renameTag(repoPath, name, newName));
    })();
  }

  function handleCheckoutLocalBranch(name: string) {
    void runAction(() => checkoutBranch(repoPath, name));
  }

  function handleCheckoutRemoteBranch(name: string) {
    void runAction(() => checkoutRemoteBranch(repoPath, name));
  }

  function handleDeleteBranch(name: string) {
    void (async () => {
      if (!(await confirmAsync(`Delete branch "${name}"?`))) return;
      await runAction(() => deleteBranch(repoPath, name));
    })();
  }

  function handleMerge(branchName: string) {
    void runAction(async () => {
      const outcome = await mergeBranch(repoPath, branchName);
      actionMessage = describeMergeOutcome(branchName, outcome);
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function describeRebaseOutcome(outcome: RebaseOutcome): string {
    switch (outcome.kind) {
      case "completed":
        return "Rebase completed.";
      case "conflicts":
        return `Rebase paused with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function handleRebase(onto: string) {
    void runAction(async () => {
      const outcome = await rebaseBranch(repoPath, onto);
      actionMessage = describeRebaseOutcome(outcome);
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function describeCherryPickOutcome(outcome: CherryPickOutcome): string {
    switch (outcome.kind) {
      case "cherry_picked":
        return "Cherry-picked onto the current branch.";
      case "conflicts":
        return `Cherry-pick stopped with ${outcome.conflicts.length} conflicting file(s).`;
    }
  }

  function handleCherryPick(commit: CommitRow) {
    void runAction(async () => {
      const outcome = await cherryPickCommit(repoPath, commit.oid);
      actionMessage = describeCherryPickOutcome(outcome);
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleMoveTag(tagName: string, onto: string) {
    void runAction(async () => {
      await moveTag(repoPath, tagName, onto);
      actionMessage = `Moved tag ${tagName} to ${onto}.`;
    });
  }

  function handleReset(onto: string, mode: ResetMode) {
    void runAction(async () => {
      await resetTo(repoPath, onto, mode);
      actionMessage = `Reset current branch to ${onto} (${mode}).`;
    });
  }

  // The drag itself only decides *that* this is a reset (`dragAction.ts`) — soft/mixed/hard
  // still needs a mode pick, made here via the same `openContextMenu` every right-click menu
  // already uses, positioned at the drop point rather than as a new dialog type.
  function handleResetDrop(onto: string, x: number, y: number) {
    openContextMenu(x, y, [
      { label: "Soft reset to this commit", onSelect: () => handleReset(onto, "soft") },
      { label: "Mixed reset to this commit", onSelect: () => handleReset(onto, "mixed") },
      {
        label: "Hard reset to this commit",
        onSelect: () => handleReset(onto, "hard"),
        danger: true,
      },
    ]);
  }

  function handlePush(branchName: string) {
    void runAction(async () => {
      try {
        await pushRemote(repoPath, REMOTE_NAME, branchName, false);
        notifySuccess(`Pushed ${branchName} to ${REMOTE_NAME}.`);
      } catch (err) {
        const message = String(err);
        if (
          !REJECTED_PUSH_PATTERN.test(message) ||
          !(await confirmAsync(
            `Push rejected: ${REMOTE_NAME} has changes you don't have locally. Force push "${branchName}" anyway? This can overwrite remote history.`,
          ))
        ) {
          throw err;
        }
        await pushRemote(repoPath, REMOTE_NAME, branchName, true);
        notifySuccess(`Force-pushed ${branchName} to ${REMOTE_NAME}.`);
      }
    });
  }

  function handleFetch() {
    void runAction(async () => {
      await fetchRemote(repoPath, REMOTE_NAME);
      actionMessage = `Fetched from ${REMOTE_NAME}.`;
    });
  }

  function handlePull() {
    void runAction(async () => {
      const outcome = await pullRemote(repoPath, REMOTE_NAME);
      actionMessage = describePullOutcome(outcome);
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handleApplyStash(index: number) {
    void runAction(async () => {
      await applyStash(repoPath, index);
      onApplied?.();
    });
  }

  function handlePopStash(index: number) {
    void runAction(async () => {
      await popStash(repoPath, index);
      onApplied?.();
    });
  }

  function handleDropStash(index: number, message: string) {
    void (async () => {
      if (!(await confirmAsync(`Drop stash "${message}"? This can't be undone.`))) return;
      await runAction(() => dropStash(repoPath, index));
    })();
  }

  // Resolves whatever's under (clientX, clientY) to a drag target the same way `handleContextMenu`
  // resolves a right-click: badge first (more specific/nested), else the owning row. Used from
  // `handlePointerMove`, not `pointerover`/`pointerout` — those boundary events don't fire for
  // elements other than the one holding pointer capture, so real hit-testing via
  // `elementFromPoint` is what's actually reliable here.
  function resolveDragTarget(clientX: number, clientY: number): DragTarget | null {
    const hit = document.elementFromPoint(clientX, clientY);
    if (!hit) return null;

    const badge = hit.closest<HTMLElement>("[data-ref-name]");
    if (badge?.dataset.refName) {
      const row = badge.closest<HTMLElement>("[data-oid]");
      const commit = row?.dataset.oid ? rows.find((r) => r.oid === row.dataset.oid) : undefined;
      const ref = commit?.refs.find(
        (r) => r.name === badge.dataset.refName && r.kind === badge.dataset.refKind,
      );
      return ref ? { type: "ref", ref } : null;
    }

    const row = hit.closest<HTMLElement>("[data-oid]");
    if (!row?.dataset.oid) return null;
    // The workdir row has no real oid to rebase onto, and a stash isn't a meaningful rebase
    // target either — neither is a valid drop target for the current branch's badge.
    const commit = rows.find((r) => r.oid === row.dataset.oid);
    if (commit && commit.kind !== "commit") return null;
    return { type: "commit", oid: row.dataset.oid };
  }

  function dragTargetKey(target: DragTarget | null): string {
    if (!target) return "";
    return target.type === "ref"
      ? `ref:${target.ref.name}:${target.ref.kind}`
      : `commit:${target.oid}`;
  }

  function describeDragAction(action: DragActionResult): string {
    if (!action) return "";
    switch (action.verb) {
      case "merge":
        return `Merge ${action.sourceName} into current branch`;
      case "rebase":
        return `Rebase current branch onto ${action.onto}`;
      case "reset":
        return `Reset current branch to ${action.onto} (pick mode on drop)`;
      case "move_tag":
        return `Move tag ${action.tagName} to ${action.onto}`;
    }
  }

  function handlePointerDown(event: PointerEvent) {
    // Left/primary button only: the drag-to-merge/rebase/move-tag gesture this feeds is
    // exclusively a left-click drag. Capturing the
    // pointer on a right-button pointerdown also has a real cross-browser side effect beyond
    // just being semantically wrong — it suppresses the `contextmenu` event that would
    // otherwise fire on release, silently breaking every ref badge's right-click menu.
    if (event.button !== 0 || busy) return;
    const badge = (event.target as HTMLElement).closest<HTMLElement>("[data-ref-name]");
    if (!badge?.dataset.refName) return;
    const row = badge.closest<HTMLElement>("[data-oid]");
    if (!row?.dataset.oid) return;
    const commit = rows.find((r) => r.oid === row.dataset.oid);
    if (!commit) return;
    const ref = commit.refs.find(
      (r) => r.name === badge.dataset.refName && r.kind === badge.dataset.refKind,
    );
    if (!ref) return;

    pointerDownInfo = { x: event.clientX, y: event.clientY, ref, commitOid: commit.oid };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent) {
    if (!pointerDownInfo) return;

    if (!dragSource) {
      const distance = Math.hypot(
        event.clientX - pointerDownInfo.x,
        event.clientY - pointerDownInfo.y,
      );
      if (distance < DRAG_THRESHOLD_PX) return;
      dragSource = { ref: pointerDownInfo.ref, commitOid: pointerDownInfo.commitOid };
    }

    dragPointerPos = { x: event.clientX, y: event.clientY };
    dragModifierHeld = event.altKey;

    const candidate = resolveDragTarget(event.clientX, event.clientY);
    if (dragTargetKey(candidate) !== dragTargetKey(dropTarget)) {
      dropTarget = candidate;
    }
  }

  function handlePointerUp(event: PointerEvent) {
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);

    const source = dragSource;
    if (source) {
      suppressNextClick = true;
      const action = dragAction;
      // Re-derive from live state rather than trusting the captured `source` reference is still
      // valid — a branch could have been deleted mid-drag by an external actor/watcher.
      const stillExists = rows.some((r) =>
        r.refs.some((ref) => ref.name === source.ref.name && ref.kind === source.ref.kind),
      );
      if (action && stillExists) {
        if (action.verb === "merge") handleMerge(action.sourceName);
        else if (action.verb === "rebase") handleRebase(action.onto);
        else if (action.verb === "reset")
          handleResetDrop(action.onto, event.clientX, event.clientY);
        else handleMoveTag(action.tagName, action.onto);
      }
    }

    pointerDownInfo = null;
    dragSource = null;
    dropTarget = null;
    dragPointerPos = null;
    dragModifierHeld = false;
  }

  function buildCommitMenu(commit: CommitRow): ContextMenuItem[] {
    return [
      { label: "Checkout this commit", onSelect: () => handleCheckoutCommit(commit) },
      { label: "Create branch here", onSelect: () => handleCreateBranchHere(commit) },
      { label: "Create tag here", onSelect: () => handleCreateTagHere(commit) },
      { label: "Rebase current branch onto this commit", onSelect: () => handleRebase(commit.oid) },
      { label: "Cherry-pick this commit", onSelect: () => handleCherryPick(commit) },
      { separator: true },
      { label: "Copy full SHA", onSelect: () => void copyText(commit.oid) },
      { label: "Copy summary", onSelect: () => void copyText(commit.summary) },
      { separator: true },
      { label: "Diff against working tree", onSelect: () => onDiffRequest?.(commit, "workdir") },
      { label: "Diff against HEAD", onSelect: () => onDiffRequest?.(commit, "head") },
    ];
  }

  // A stash row's own context menu — apply/pop/drop, reusing the exact commands
  // `StashPanel.svelte`'s equivalent buttons already call.
  function buildStashMenu(commit: CommitRow): ContextMenuItem[] {
    if (commit.stashIndex === null) return [];
    const index = commit.stashIndex;
    return [
      { label: "Apply stash", onSelect: () => handleApplyStash(index) },
      { label: "Pop stash", onSelect: () => handlePopStash(index) },
      { separator: true },
      { label: "Drop stash", danger: true, onSelect: () => handleDropStash(index, commit.summary) },
    ];
  }

  // Tag badges were originally scoped out of the graph's
  // context menu entirely; reversed on explicit user request to expose delete/rename here
  // too, reusing the same `delete_tag`/`rename_tag` commands `TagsPanel.svelte` and the
  // drag-to-move-tag gesture already call.
  function buildRefMenu(ref: RefMarker): ContextMenuItem[] {
    if (ref.kind === "tag") {
      return [
        { label: "Rename tag", onSelect: () => handleRenameTag(ref.name) },
        { label: "Delete tag", danger: true, onSelect: () => handleDeleteTag(ref.name) },
        { separator: true },
        { label: `Push tag to ${REMOTE_NAME}`, onSelect: () => handlePush(ref.name) },
      ];
    }

    if (ref.kind === "remote_branch") {
      return [
        {
          label: "Checkout (creates local branch)",
          onSelect: () => handleCheckoutRemoteBranch(ref.name),
        },
        { separator: true },
        { label: `Fetch ${REMOTE_NAME}`, onSelect: () => handleFetch() },
        { label: "Pull current branch", onSelect: () => handlePull() },
        { separator: true },
        { label: "Merge into current branch", onSelect: () => handleMerge(ref.name) },
        { label: `Rebase current branch onto ${ref.name}`, onSelect: () => handleRebase(ref.name) },
      ];
    }

    const items: ContextMenuItem[] = [];
    if (!ref.isHead) {
      items.push(
        { label: "Checkout branch", onSelect: () => handleCheckoutLocalBranch(ref.name) },
        { label: "Merge into current branch", onSelect: () => handleMerge(ref.name) },
        {
          label: `Rebase current branch onto ${ref.name}`,
          onSelect: () => handleRebase(ref.name),
        },
        { label: "Delete branch", danger: true, onSelect: () => handleDeleteBranch(ref.name) },
        { separator: true },
      );
    }
    items.push({ label: `Push to ${REMOTE_NAME}`, onSelect: () => handlePush(ref.name) });
    return items;
  }

  // Resolve the target the same way `handleClick` does,
  // then branch by whether the closest match is a ref badge or the commit row itself. A ref
  // badge is nested inside its row's `[data-oid]`, so it's checked first.
  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    const target = (event.target as HTMLElement).closest<HTMLElement>("[data-oid]");
    if (!target?.dataset.oid) return;
    const commit = rows.find((r) => r.oid === target.dataset.oid);
    if (!commit) return;
    select(commit);

    // Neither row kind has the usual commit/ref menus: the workdir row isn't a real commit
    // (nothing to checkout/branch/diff-against), and a stash gets its own apply/pop/drop menu.
    if (commit.kind === "workdir") return;
    if (commit.kind === "stash") {
      openContextMenu(event.clientX, event.clientY, buildStashMenu(commit));
      return;
    }

    const badge = (event.target as HTMLElement).closest<HTMLElement>("[data-ref-name]");
    if (badge?.dataset.refName) {
      // A branch and a tag can legitimately share a name at the same commit, so match on both
      // the name and the kind, not the name alone.
      const ref = commit.refs.find(
        (r) => r.name === badge.dataset.refName && r.kind === badge.dataset.refKind,
      );
      const items = ref ? buildRefMenu(ref) : [];
      if (items.length === 0) return;
      openContextMenu(event.clientX, event.clientY, items);
      return;
    }

    openContextMenu(event.clientX, event.clientY, buildCommitMenu(commit));
  }

  // Arrow up/down moves the selection by row ± 1 and
  // scrolls the virtualized list to keep it in view.
  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    if (rows.length === 0) return;
    event.preventDefault();

    const currentIndex = rows.findIndex((r) => r.oid === selectedOid);
    const delta = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = Math.min(rows.length - 1, Math.max(0, currentIndex + delta));
    const next = rows[nextIndex];
    if (!next) return;
    select(next);

    if (!containerEl) return;
    const rowTop = nextIndex * ROW_HEIGHT;
    const rowBottom = rowTop + ROW_HEIGHT;
    if (rowTop < containerEl.scrollTop) {
      containerEl.scrollTop = rowTop;
    } else if (rowBottom > containerEl.scrollTop + containerEl.clientHeight) {
      containerEl.scrollTop = rowBottom - containerEl.clientHeight;
    }
    scrollTop = containerEl.scrollTop;
    maybeLoadMore(containerEl);
  }

  function formatDate(unixSeconds: number): string {
    return new Date(unixSeconds * 1000).toLocaleDateString();
  }
</script>

<div
  class="graph-scroll"
  class:dragging={dragSource !== null}
  role="listbox"
  aria-label="Commit rows"
  aria-activedescendant={selectedOid ? `commit-${selectedOid}` : undefined}
  tabindex="0"
  bind:this={containerEl}
  bind:clientHeight
  onscroll={handleScroll}
  onclick={handleClick}
  onkeydown={handleKeydown}
  oncontextmenu={handleContextMenu}
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
>
  {#if error}
    <p class="graph-error" role="alert">{error}</p>
  {:else if !repoPath}
    <p class="graph-empty">Open a repository to see its commit graph.</p>
  {/if}
  {#if actionError}
    <p class="action-banner error" role="alert">{actionError}</p>
  {:else if actionMessage}
    <div class="action-banner">
      <p>{actionMessage}</p>
    </div>
  {/if}
  <div class="graph-content" style:height="{contentHeight}px">
    {#each visibleRows as commit (commit.oid)}
      <div
        id="commit-{commit.oid}"
        class="row"
        class:selected={commit.oid === selectedOid}
        class:workdir-row={commit.kind === "workdir"}
        class:drop-target={dropTarget?.type === "commit" && dropTarget.oid === commit.oid}
        data-oid={commit.oid}
        role="option"
        aria-selected={commit.oid === selectedOid}
        style:transform="translateY({commit.row * ROW_HEIGHT}px)"
        style:height="{ROW_HEIGHT}px"
      >
        <svg class="gutter" width={gutterWidth} height={ROW_HEIGHT} aria-hidden="true">
          {#each commit.rails as rail}
            <path
              d={railPath(rail)}
              stroke={colorFor(rail.colorId)}
              stroke-width="2"
              fill="none"
              data-color-id={rail.colorId}
            />
          {/each}
          {#if commit.kind === "workdir"}
            <circle
              cx={laneX(commit.lane)}
              cy={ROW_HEIGHT / 2}
              r={DOT_RADIUS}
              fill="none"
              stroke={colorFor(commit.colorId)}
              stroke-width="2"
              stroke-dasharray="2,2"
            />
          {:else}
            <circle
              cx={laneX(commit.lane)}
              cy={ROW_HEIGHT / 2}
              r={DOT_RADIUS}
              fill={colorFor(commit.colorId)}
            />
          {/if}
        </svg>
        {#if commit.kind === "workdir"}
          <div class="summary workdir-summary">{commit.summary}</div>
        {:else}
          <div class="refs">
            {#if commit.kind === "stash"}
              <span class="ref-badge stash-badge">stash@{"{"}{commit.stashIndex}{"}"}</span>
            {/if}
            {#each commit.refs as ref}
              <span
                class="ref-badge draggable"
                class:head={ref.isHead}
                class:drop-target={dropTarget?.type === "ref" &&
                  dropTarget.ref.name === ref.name &&
                  dropTarget.ref.kind === ref.kind}
                data-ref-name={ref.name}
                data-ref-kind={ref.kind}>{ref.name}</span
              >
            {/each}
          </div>
          <div class="summary" title={commit.summary}>{commit.summary}</div>
          <div class="meta">
            <Avatar name={commit.authorName} email={commit.authorEmail} />
            <span class="oid" title={commit.oid}>{commit.shortOid}</span>
            <CopyButton text={commit.oid} label="Copy commit SHA" />
            <span class="date">{formatDate(commit.authorTime)}</span>
          </div>
        {/if}
      </div>
    {/each}
  </div>
  {#if showLoading}
    <p class="graph-loading">Loading…</p>
  {/if}
  {#if dragAction && dragPointerPos}
    <div class="drag-label" style:left="{dragPointerPos.x}px" style:top="{dragPointerPos.y}px">
      {describeDragAction(dragAction)}
    </div>
  {/if}
</div>

<style>
  .graph-scroll {
    position: relative;
    height: 100%;
    overflow: auto;
    /* The graph has no text-selection interaction of its own (SHAs/summaries are copied via
       the dedicated copy button, not click-drag) — unconditional, not just while `.dragging`,
       since a drag started elsewhere (a badge, a pane's resize handle) still passes the mouse
       over graph rows and would otherwise highlight their text as a side effect.
       `-webkit-user-select` is the one that actually matters here: PushGit's real target is
       WebKitGTK, and unprefixed
       `user-select` alone is unreliable there. */
    -webkit-user-select: none;
    user-select: none;
  }

  /* While a drag is in flight, the captured element's own `cursor` is what the browser
     honors during a captured pointer interaction, regardless of what's visually underneath. */
  .graph-scroll.dragging {
    cursor: grabbing;
  }

  .graph-content {
    position: relative;
    width: 100%;
  }

  .graph-error,
  .graph-empty,
  .graph-loading {
    padding: 0.5rem 1rem;
    color: var(--text-muted);
  }

  .graph-error {
    color: var(--danger);
  }

  .action-banner {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    margin: 0;
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    background: var(--surface-1);
    border-bottom: 1px solid var(--border);
  }

  .action-banner p {
    margin: 0;
  }

  .action-banner.error {
    color: var(--danger);
    background: var(--danger-bg);
  }

  .row {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 0.5rem;
    cursor: pointer;
    white-space: nowrap;
    transition: background-color 0.1s ease;
  }

  .row:hover {
    background: var(--surface-2);
  }

  .row.selected,
  .row.selected:hover {
    background: var(--accent-bg);
  }

  .row.drop-target {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .row.workdir-row {
    font-style: italic;
    color: var(--text-secondary);
  }

  .workdir-summary {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .gutter {
    flex-shrink: 0;
  }

  .refs {
    display: flex;
    gap: 0.25rem;
    flex-shrink: 0;
  }

  .ref-badge {
    font-size: 0.7rem;
    padding: 0.05rem 0.5rem;
    border-radius: var(--radius-lg);
    color: var(--text-secondary);
    background: var(--surface-2);
    border: 1px solid var(--border);
  }

  .ref-badge.head {
    font-weight: 600;
    color: var(--accent);
    border-color: var(--accent);
  }

  .ref-badge.draggable {
    cursor: grab;
  }

  .ref-badge.drop-target {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .ref-badge.stash-badge {
    color: var(--warning);
    border-color: var(--warning);
  }

  .drag-label {
    position: fixed;
    z-index: 20;
    /* Critical, not optional: `elementFromPoint` (used for drop-target hit-testing on every
       pointermove) returns the topmost element at the point — without this, the label itself
       would shadow the real row/badge underneath it and silently break drop-target resolution
       the instant it becomes visible. */
    pointer-events: none;
    transform: translate(12px, 12px);
    padding: 0.3rem 0.6rem;
    font-size: 0.8rem;
    color: var(--surface-0);
    background: var(--accent);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    white-space: nowrap;
  }

  .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1 1 auto;
    min-width: 8rem;
  }

  .meta {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 0.8rem;
  }

  .oid {
    font-family: var(--font-mono);
  }
</style>
