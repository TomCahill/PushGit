<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Remote panel: fetch/pull/push against "origin". There's no
  // `list_remotes` backend command yet, so "origin" is assumed — the same MVP scope
  // narrowing as TagsPanel always creating at HEAD or BranchSidebar's "create branch" never
  // exposing a target commit. Pull is fetch + merge under the hood (`remote::pull`), so it
  // gets the same pre-op snapshot/undo as BranchSidebar's merge/rebase. Push only offers a force-push after a real
  // rejection (detected from the error text), confirmed via `confirmAsync()` — never
  // force-pushes silently.
  import {
    cancelRemoteOperation,
    checkGitVersion,
    fetchRemote,
    listBranches,
    pullRemote,
    pushRemote,
  } from "$lib/git/api";
  import { describePullOutcome } from "$lib/git/describeOutcome";
  import type { RemoteProgress } from "$lib/git/types";
  import { settingsState } from "$lib/settings/settings.svelte";
  import { burstConfetti } from "$lib/shell/confetti.svelte";
  import { confirmAsync } from "$lib/shell/confirmDialog.svelte";
  import Button from "$lib/shell/Button.svelte";
  import {
    finishHookOutput,
    pushHookOutputLine,
    revealHookOutput,
    startHookOutput,
  } from "$lib/shell/hookOutput.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";

  const REMOTE_NAME = "origin";
  const REJECTED_PUSH_PATTERN = /rejected|non-fast-forward|fetch first/i;
  // Matches `PushGitError::Cancelled`'s `#[error("operation cancelled")]` message
  // (`src-tauri/src/error.rs`) — a user-initiated cancel isn't a failure, so it surfaces as a
  // success toast rather than an error one.
  const CANCELLED_PATTERN = /operation cancelled/i;

  let {
    repoPath,
    refreshKey,
    onChanged,
    onConflicts,
    onBusyChange,
    onProgressChange,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onConflicts?: () => void;
    /** Lets `ActionRail` render fetch/pull/push progress as the graph's top border bar. */
    onBusyChange?: (busy: boolean) => void;
    onProgressChange?: (progress: RemoteProgress | null) => void;
  } = $props();

  let versionWarning = $state<string | null>(null);
  let currentBranchName = $state<string | null>(null);
  let hasUpstream = $state(false);
  let ahead = $state(0);
  let behind = $state(0);

  let busy = $state(false);
  let progress = $state<RemoteProgress | null>(null);
  let pushButtonEl = $state<HTMLButtonElement | null>(null);

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  $effect(() => {
    onBusyChange?.(busy);
  });

  $effect(() => {
    onProgressChange?.(progress);
  });

  $effect(() => {
    checkGitVersion()
      .then((check) => {
        versionWarning = check.isPatched
          ? null
          : `Installed git ${check.version} predates the CVE-2024-32002 fix — network operations still work, but consider upgrading git.`;
      })
      .catch(() => {
        // Best-effort: an unparseable/missing git version shouldn't block the UI.
      });
  });

  // Periodic fetch, on by default (`SettingsPanel`'s "Auto-fetch" toggle can turn it off). Re-runs
  // whenever the repo, the toggle, or the interval changes — switching repos or turning it off
  // clears the previous interval via this effect's own teardown, so there's never more than one
  // timer alive, and never one pointed at a stale repo path.
  $effect(() => {
    const path = repoPath;
    const enabled = settingsState.autoFetchEnabled;
    const intervalMinutes = settingsState.autoFetchIntervalMinutes;
    if (!enabled || !path) return;

    const id = setInterval(() => void autoFetchTick(path), intervalMinutes * 60_000);
    return () => clearInterval(id);
  });

  /** A tick of the auto-fetch timer: skipped outright (never queued) if a manual op or a
   *  previous tick is still in flight, or the window isn't visible. Silent on success — just
   *  refreshes ahead/behind and lets the graph pick up any new remote refs via `onChanged` —
   *  and logged rather than toasted on failure, since "could not resolve host" every few
   *  minutes while offline is noise, not signal. */
  async function autoFetchTick(path: string) {
    if (busy || document.visibilityState !== "visible") return;
    busy = true;
    progress = null;
    try {
      await fetchRemote(path, REMOTE_NAME, onProgress);
      await reload(path, refreshKey);
      onChanged?.();
    } catch (err) {
      console.error("Auto-fetch failed:", err);
    } finally {
      busy = false;
      progress = null;
    }
  }

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      currentBranchName = null;
      hasUpstream = false;
      ahead = 0;
      behind = 0;
      return;
    }

    try {
      const branches = await listBranches(path);
      if (myGeneration !== generation) return;
      const head = branches.find((b) => b.isHead) ?? null;
      currentBranchName = head?.name ?? null;
      hasUpstream = Boolean(head?.upstream);
      ahead = head?.ahead ?? 0;
      behind = head?.behind ?? 0;
    } catch {
      // BranchSidebar already surfaces branch-load errors; no need to duplicate that here.
    }
  }

  async function runAction(fn: () => Promise<void>) {
    if (busy) return;
    busy = true;
    progress = null;
    try {
      await fn();
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      const message = String(err);
      if (CANCELLED_PATTERN.test(message)) {
        notifySuccess("Cancelled.");
      } else {
        notifyError(message);
      }
    } finally {
      busy = false;
      progress = null;
    }
  }

  function handleCancel() {
    void cancelRemoteOperation(repoPath);
  }

  function onProgress(update: RemoteProgress) {
    progress = update;
  }

  function progressLabel(p: RemoteProgress): string {
    return p.current != null && p.total != null
      ? `${p.phase}: ${p.percent}% (${p.current}/${p.total})`
      : `${p.phase}: ${p.percent}%`;
  }

  function handleFetch() {
    void runAction(async () => {
      await fetchRemote(repoPath, REMOTE_NAME, onProgress);
      notifySuccess(`Fetched from ${REMOTE_NAME}.`);
    });
  }

  function handlePull() {
    void runAction(async () => {
      const outcome = await pullRemote(repoPath, REMOTE_NAME, onProgress);
      notifySuccess(describePullOutcome(outcome));
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }

  function handlePush() {
    const branchName = currentBranchName;
    if (!branchName) return;
    void runAction(async () => {
      startHookOutput("Push", settingsState.showHookOutputAlways);
      try {
        try {
          await pushRemote(
            repoPath,
            REMOTE_NAME,
            branchName,
            false,
            onProgress,
            pushHookOutputLine,
          );
          notifySuccess(`Pushed ${branchName} to ${REMOTE_NAME}.`);
          if (pushButtonEl) burstConfetti(pushButtonEl.getBoundingClientRect());
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
          await pushRemote(repoPath, REMOTE_NAME, branchName, true, onProgress, pushHookOutputLine);
          notifySuccess(`Force-pushed ${branchName} to ${REMOTE_NAME}.`);
          if (pushButtonEl) burstConfetti(pushButtonEl.getBoundingClientRect());
        }
      } catch (err) {
        if (!settingsState.showHookOutputAlways) revealHookOutput();
        throw err;
      } finally {
        finishHookOutput();
      }
    });
  }
</script>

<div class="remote-panel">
  <div class="remote-actions">
    <Button variant="tonal" onclick={handleFetch} disabled={busy} title="Fetch">
      <Icon name="refresh-cw" size={13} /> Fetch
    </Button>
    <Button
      variant="tonal"
      onclick={handlePull}
      disabled={busy || !hasUpstream}
      title={hasUpstream
        ? behind > 0
          ? `Pull — ${behind} commit${behind === 1 ? "" : "s"} behind ${REMOTE_NAME}`
          : "Pull"
        : "Current branch has no upstream configured"}
    >
      <Icon name="arrow-down" size={13} /> Pull
      {#if behind > 0}
        <span class="count-badge">{behind}</span>
      {/if}
    </Button>
    <Button
      variant="filled"
      bind:ref={pushButtonEl}
      onclick={handlePush}
      disabled={busy || !currentBranchName}
      title={ahead > 0
        ? `Push — ${ahead} commit${ahead === 1 ? "" : "s"} ahead of ${REMOTE_NAME}`
        : "Push"}
    >
      <Icon name="arrow-up" size={13} /> Push
      {#if ahead > 0}
        <span class="count-badge">{ahead}</span>
      {/if}
    </Button>
    {#if busy}
      <span class="cancel-wrap">
        <Button variant="outlined" onclick={handleCancel} title="Cancel">
          <Icon name="x" size={13} /> Cancel
        </Button>
      </span>
    {/if}
  </div>

  {#if busy && progress}
    <p class="progress-label" role="status">{progressLabel(progress)}</p>
  {/if}

  {#if versionWarning}
    <p class="warning" role="alert">{versionWarning}</p>
  {/if}
</div>

<style>
  .remote-panel {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .warning {
    margin: 0;
    padding: 0.35rem 0.5rem;
    font-size: 0.72rem;
    color: var(--text-primary);
    background: var(--warning-bg);
    border: 1px solid var(--warning);
    border-radius: var(--radius-sm);
  }

  .remote-actions {
    display: flex;
    gap: 0.3rem;
  }

  .remote-actions :global(.btn) {
    font-size: 0.8rem;
    padding: 0.4rem 0.9rem;
  }

  .cancel-wrap :global(.btn-outlined) {
    color: var(--danger);
    border-color: var(--danger);
  }

  .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 1.1rem;
    height: 1.1rem;
    padding: 0 0.25rem;
    font-size: 0.68rem;
    font-weight: 600;
    line-height: 1;
    color: var(--accent);
    background: var(--accent-bg);
    border-radius: 999px;
  }

  :global(.btn-filled) .count-badge {
    color: var(--btn-filled-bg);
    background: var(--btn-filled-fg);
  }

  .progress-label {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-secondary);
    white-space: nowrap;
  }
</style>
