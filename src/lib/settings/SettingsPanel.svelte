<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // App-wide settings ("Max-commits render limit (perf setting)") plus,
  // when a repo is open, per-repo settings — the
  // only "This Repository" section content so far is stuff that used to be ephemeral,
  // per-action choices in `StagingPanel.svelte`: skip-hooks-by-default and the commit message
  // template (the latter reads/writes git's own `commit.template` key directly, not an
  // app-owned copy of it).
  import {
    detectWorkflow,
    getCommitTemplatePath,
    getRepoConfig,
    initWorkflow,
    setCommitTemplatePath,
    setRepoDefaultSkipHooks,
  } from "$lib/git/api";
  import { setMaxCommitsRendered, setReduceMotion, settingsState } from "./settings.svelte";
  import { notifyError } from "$lib/shell/toast.svelte";
  import type { WorkflowConfig } from "$lib/git/types";

  let { repoPath = null }: { repoPath?: string | null } = $props();

  let draftValue = $state(String(settingsState.maxCommitsRendered));
  let saved = $state(false);

  let defaultSkipHooks = $state(false);
  let commitTemplatePath = $state("");
  let repoSaved = $state(false);

  let workflowConfig = $state<WorkflowConfig | null>(null);
  let workflowMain = $state("main");
  let workflowDevelop = $state("develop");
  let workflowFeaturePrefix = $state("feature/");
  let workflowReleasePrefix = $state("release/");
  let workflowHotfixPrefix = $state("hotfix/");
  let workflowVersionTagPrefix = $state("");
  // Guards `loadRepoSettings`'s async response against clobbering a user edit that lands
  // before the load resolves (checkbox click, or typing in the template field) — same
  // "don't overwrite what the user already touched" concern `StagingPanel.svelte`'s
  // `prefillFromTemplate` handles for its own commit-template prefill.
  let repoSettingsDirty = false;

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    const parsed = Number(draftValue);
    if (!Number.isFinite(parsed)) return;
    try {
      await setMaxCommitsRendered(parsed);
      draftValue = String(settingsState.maxCommitsRendered);
      saved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  function handleInput() {
    saved = false;
  }

  async function loadRepoSettings(path: string) {
    if (!path) return;
    try {
      const [config, templatePath] = await Promise.all([
        getRepoConfig(path),
        getCommitTemplatePath(path),
      ]);
      if (repoSettingsDirty) return;
      defaultSkipHooks = config.defaultSkipHooks;
      commitTemplatePath = templatePath ?? "";
    } catch {
      // Leave the defaults; this is a convenience prefill only.
    }

    // Fetched independently of the settings above: a GitFlow-detection failure shouldn't
    // block the rest of this section from showing its own (unrelated) prefilled values.
    try {
      const workflow = await detectWorkflow(path);
      if (!repoSettingsDirty) workflowConfig = workflow;
    } catch {
      // Leave the "not configured" state; this is a convenience prefill only.
    }
  }

  $effect(() => {
    repoSettingsDirty = false;
    void loadRepoSettings(repoPath ?? "");
  });

  async function handleSkipHooksChange() {
    if (!repoPath) return;
    repoSettingsDirty = true;
    const value = defaultSkipHooks;
    try {
      await setRepoDefaultSkipHooks(repoPath, value);
      repoSaved = true;
    } catch (err) {
      defaultSkipHooks = !value; // revert the optimistic checkbox toggle
      notifyError(String(err));
    }
  }

  async function handleTemplateSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (!repoPath) return;
    try {
      await setCommitTemplatePath(repoPath, commitTemplatePath.trim() || null);
      repoSaved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  function handleTemplateInput() {
    repoSaved = false;
    repoSettingsDirty = true;
  }

  async function handleWorkflowSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (!repoPath) return;
    const config: WorkflowConfig = {
      main: workflowMain.trim() || "main",
      develop: workflowDevelop.trim() || "develop",
      featurePrefix: workflowFeaturePrefix.trim(),
      releasePrefix: workflowReleasePrefix.trim(),
      hotfixPrefix: workflowHotfixPrefix.trim(),
      supportPrefix: null,
      versionTagPrefix: workflowVersionTagPrefix.trim(),
    };
    try {
      await initWorkflow(repoPath, config);
      workflowConfig = config;
    } catch (err) {
      notifyError(String(err));
    }
  }

  async function handleReduceMotionChange() {
    const value = settingsState.reduceMotion;
    try {
      await setReduceMotion(value);
    } catch (err) {
      settingsState.reduceMotion = !value; // revert the optimistic checkbox toggle
      notifyError(String(err));
    }
  }
</script>

<div class="settings-panel">
  <section class="settings-section">
    <h3>Application</h3>
    <form onsubmit={handleSubmit}>
      <label for="max-commits-rendered">Max commits rendered in graph</label>
      <div class="row">
        <input
          id="max-commits-rendered"
          type="text"
          inputmode="numeric"
          bind:value={draftValue}
          oninput={handleInput}
        />
        <button type="submit">Save</button>
      </div>
      <p class="hint">
        {saved ? "Saved." : `A new page of commits loads at most this many rows at a time.`}
      </p>
    </form>

    <label class="checkbox-row">
      <input
        type="checkbox"
        bind:checked={settingsState.reduceMotion}
        onchange={handleReduceMotionChange}
      />
      Reduce motion
    </label>
  </section>

  {#if repoPath}
    <section class="settings-section">
      <h3>This Repository</h3>

      <label class="checkbox-row">
        <input type="checkbox" bind:checked={defaultSkipHooks} onchange={handleSkipHooksChange} />
        Skip hooks by default
      </label>

      <form onsubmit={handleTemplateSubmit}>
        <label for="commit-template-path">Commit message template</label>
        <div class="row">
          <input
            id="commit-template-path"
            type="text"
            placeholder="No template configured"
            bind:value={commitTemplatePath}
            oninput={handleTemplateInput}
          />
          <button type="submit">Save</button>
        </div>
        <p class="hint">
          {repoSaved
            ? "Saved."
            : "Sets this repo's git commit.template path — clear the field to unset it."}
        </p>
      </form>

      {#if workflowConfig}
        <div class="workflow-status">
          <label for="gitflow-status">GitFlow</label>
          <p id="gitflow-status" class="hint">
            Configured — main branch "{workflowConfig.main}", develop branch "{workflowConfig.develop}".
            Start/finish feature, release, and hotfix branches from the Workflow panel.
          </p>
        </div>
      {:else}
        <form class="workflow-setup" onsubmit={handleWorkflowSubmit}>
          <label for="gitflow-main">Set up GitFlow</label>
          <div class="row">
            <input
              id="gitflow-main"
              type="text"
              bind:value={workflowMain}
              placeholder="main"
              aria-label="Main branch name"
            />
            <input
              type="text"
              bind:value={workflowDevelop}
              placeholder="develop"
              aria-label="Develop branch name"
            />
          </div>
          <div class="row">
            <input
              type="text"
              bind:value={workflowFeaturePrefix}
              placeholder="feature/"
              aria-label="Feature branch prefix"
            />
            <input
              type="text"
              bind:value={workflowReleasePrefix}
              placeholder="release/"
              aria-label="Release branch prefix"
            />
            <input
              type="text"
              bind:value={workflowHotfixPrefix}
              placeholder="hotfix/"
              aria-label="Hotfix branch prefix"
            />
          </div>
          <div class="row">
            <input
              type="text"
              bind:value={workflowVersionTagPrefix}
              placeholder="Version tag prefix (blank for none)"
              aria-label="Version tag prefix"
            />
            <button type="submit">Set up</button>
          </div>
          <p class="hint">
            Reads/writes the same .git/config keys as the git-flow CLI, so a repo set up here also
            works with git flow directly.
          </p>
        </form>
      {/if}
    </section>
  {/if}
</div>

<style>
  .settings-panel {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    min-width: 14rem;
  }

  .settings-section {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .settings-section h3 {
    margin: 0;
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    color: var(--text-muted);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .workflow-status,
  .workflow-setup {
    margin-top: 0.25rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .workflow-status {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.8rem;
    color: var(--text-primary);
  }

  .row {
    display: flex;
    gap: 0.35rem;
  }

  .row input {
    flex: 1 1 auto;
    min-width: 0;
    font: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.5rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .row input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .row button {
    font-size: 0.8rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .row button:hover {
    background: var(--surface-2);
  }

  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
</style>
