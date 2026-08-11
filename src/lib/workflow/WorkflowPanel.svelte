<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // GitFlow start/finish panel: Feature/Release/Hotfix sections, each listing in-progress
  // branches of that kind (derived client-side from `listBranches` by filtering on the
  // configured prefix — no dedicated listing command needed) with a "Start" action per
  // section and a "Finish" action per listed branch. The one-time "Set up GitFlow" form
  // lives in `SettingsPanel.svelte` instead of here — see the git-workflows plan for why.
  // Finish conflicts route through the exact same conflict banner and 3-way editor already
  // used by plain merge: `onConflicts` tells the parent to switch to that view, and the
  // caller resolves + commits there, then clicks "Finish" again to pick the remaining steps
  // back up (`finish_branch` is idempotent on replay).
  import {
    detectWorkflow,
    finishWorkflowBranch,
    listBranches,
    startWorkflowBranch,
  } from "$lib/git/api";
  import { promptAsync } from "$lib/shell/confirmDialog.svelte";
  import { createReloadable } from "$lib/shell/reloadable.svelte";
  import { notifyError, notifySuccess } from "$lib/shell/toast.svelte";
  import type {
    BranchInfo,
    FinishOutcome,
    WorkflowBranchKind,
    WorkflowConfig,
  } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onConflicts,
  }: {
    repoPath: string;
    refreshKey: number;
    onChanged?: () => void;
    onConflicts?: () => void;
  } = $props();

  const sections: { kind: WorkflowBranchKind; label: string }[] = [
    { kind: "feature", label: "Feature" },
    { kind: "release", label: "Release" },
    { kind: "hotfix", label: "Hotfix" },
  ];

  let workflowConfig = $state<WorkflowConfig | null>(null);
  let branches = $state<BranchInfo[]>([]);
  let loadError = $state<string | null>(null);

  const workflowReload = createReloadable(async (path, isStale) => {
    if (!path) {
      workflowConfig = null;
      branches = [];
      return;
    }

    loadError = null;
    try {
      const [config, branchList] = await Promise.all([detectWorkflow(path), listBranches(path)]);
      if (isStale()) return;
      workflowConfig = config;
      branches = branchList;
    } catch (err) {
      if (!isStale()) {
        loadError = String(err);
        notifyError(loadError);
      }
    }
  });

  let busy = $derived(workflowReload.busy);

  $effect(() => {
    void workflowReload.reload(repoPath, refreshKey);
  });

  function runAction(fn: () => Promise<void>) {
    void workflowReload.runAction(repoPath, fn, notifyError, onChanged);
  }

  function prefixFor(config: WorkflowConfig, kind: WorkflowBranchKind): string {
    switch (kind) {
      case "feature":
        return config.featurePrefix;
      case "release":
        return config.releasePrefix;
      case "hotfix":
        return config.hotfixPrefix;
    }
  }

  function branchesFor(kind: WorkflowBranchKind): { name: string; suffix: string }[] {
    if (!workflowConfig) return [];
    const prefix = prefixFor(workflowConfig, kind);
    if (!prefix) return [];
    return branches
      .filter((b) => b.name.startsWith(prefix))
      .map((b) => ({ name: b.name, suffix: b.name.slice(prefix.length) }));
  }

  function describeFinishOutcome(name: string, outcome: FinishOutcome): string {
    return outcome.kind === "finished"
      ? `Finished "${name}".`
      : `Finish stopped with ${outcome.conflicts.length} conflicting file(s).`;
  }

  async function handleStart(kind: WorkflowBranchKind, label: string) {
    if (!workflowConfig) return;
    const name = (await promptAsync(`New ${label.toLowerCase()} branch name:`))?.trim();
    if (!name) return;
    void runAction(async () => {
      await startWorkflowBranch(repoPath, workflowConfig!, kind, name);
      notifySuccess(`Started "${prefixFor(workflowConfig!, kind)}${name}".`);
    });
  }

  function handleFinish(kind: WorkflowBranchKind, suffix: string) {
    if (!workflowConfig) return;
    void runAction(async () => {
      const outcome = await finishWorkflowBranch(repoPath, workflowConfig!, kind, suffix);
      notifySuccess(describeFinishOutcome(suffix, outcome));
      if (outcome.kind === "conflicts") onConflicts?.();
    });
  }
</script>

<div class="workflow-panel">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  {#if !workflowConfig}
    <p class="placeholder">GitFlow isn't set up for this repo — configure it in Settings.</p>
  {:else}
    {#each sections as section (section.kind)}
      <section class="workflow-section">
        <div class="section-header">
          <h4>{section.label}</h4>
          <button
            type="button"
            onclick={() => handleStart(section.kind, section.label)}
            disabled={busy}
          >
            Start
          </button>
        </div>

        {#if branchesFor(section.kind).length === 0}
          <p class="placeholder">None in progress.</p>
        {:else}
          <ul class="branch-list">
            {#each branchesFor(section.kind) as branch (branch.name)}
              <li>
                <span class="branch-name">{branch.suffix}</span>
                <button
                  type="button"
                  title={`Finish ${branch.name}`}
                  onclick={() => handleFinish(section.kind, branch.suffix)}
                  disabled={busy}
                >
                  Finish
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
  {/if}
</div>

<style>
  .workflow-panel {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-width: 14rem;
  }

  .placeholder {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  .error {
    color: var(--danger);
    font-size: 0.85rem;
  }

  .workflow-section {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .workflow-section:not(:first-child) {
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .section-header h4 {
    margin: 0;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
  }

  .section-header button {
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .section-header button:not(:disabled):hover {
    background: var(--surface-2);
  }

  .branch-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .branch-list li {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.15rem 0.2rem;
    border-radius: var(--radius-sm);
  }

  .branch-list li:hover {
    background: var(--surface-2);
  }

  .branch-name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .branch-list button {
    flex-shrink: 0;
    font-size: 0.7rem;
    padding: 0.2rem 0.4rem;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .branch-list button:hover {
    background: var(--surface-2);
  }
</style>
