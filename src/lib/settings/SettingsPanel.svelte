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
  import {
    clearAiApiKey,
    setAiApiKey,
    setAiInstructions,
    setAiTransport,
    setMaxCommitsRendered,
    setReduceMotion,
    settingsState,
  } from "./settings.svelte";
  import { notifyError } from "$lib/shell/toast.svelte";
  import type { AiTransport, WorkflowConfig } from "$lib/git/types";

  const ANTHROPIC_DEFAULT_BASE_URL = "https://api.anthropic.com";

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

  // AI provider/model/instructions/key — the single place any of this feature's
  // configuration is surfaced (the commit box's "Generate with AI" button carries none of
  // it). `aiProviderKind`/`aiBaseUrl`/`aiModel` are drafted from `settingsState.ai.transport`
  // once at mount, the same "local draft + explicit Save" pattern as the commit-template
  // field above, rather than two-way binding straight to shared state like the plain
  // checkboxes do — a provider/URL/model change should only take effect together, on submit.
  type AiProviderKind = "none" | AiTransport["kind"];
  let aiProviderKind = $state<AiProviderKind>(settingsState.ai.transport?.kind ?? "none");
  let aiBaseUrl = $state(settingsState.ai.transport?.baseUrl ?? "");
  let aiModel = $state(settingsState.ai.transport?.model ?? "");
  let aiTransportSaved = $state(false);

  let aiInstructions = $state(settingsState.ai.instructions);
  let aiInstructionsSaved = $state(false);

  let aiApiKeyDraft = $state("");
  let aiApiKeySaved = $state(false);

  function handleAiProviderChange() {
    aiTransportSaved = false;
    // No default model name for any provider (availability varies too much per local
    // install/account tier to guess safely) — but Anthropic's base URL defaults to its own
    // API, editable in case the user runs a compatible proxy.
    if (aiProviderKind === "anthropic" && aiBaseUrl.trim() === "") {
      aiBaseUrl = ANTHROPIC_DEFAULT_BASE_URL;
    }
  }

  function handleAiTransportInput() {
    aiTransportSaved = false;
  }

  async function handleAiTransportSubmit(event: SubmitEvent) {
    event.preventDefault();
    const transport: AiTransport | null =
      aiProviderKind === "none"
        ? null
        : { kind: aiProviderKind, baseUrl: aiBaseUrl.trim(), model: aiModel.trim() };
    try {
      await setAiTransport(transport);
      aiTransportSaved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  function handleAiInstructionsInput() {
    aiInstructionsSaved = false;
  }

  async function handleAiInstructionsSubmit(event: SubmitEvent) {
    event.preventDefault();
    try {
      await setAiInstructions(aiInstructions);
      aiInstructionsSaved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  function handleAiApiKeyInput() {
    aiApiKeySaved = false;
  }

  async function handleAiApiKeySubmit(event: SubmitEvent) {
    event.preventDefault();
    if (!aiApiKeyDraft.trim()) return;
    try {
      await setAiApiKey(aiApiKeyDraft.trim());
      aiApiKeyDraft = ""; // write-only field — never reflect the saved value back
      aiApiKeySaved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  async function handleAiApiKeyClear() {
    try {
      await clearAiApiKey();
      aiApiKeyDraft = "";
      aiApiKeySaved = false;
    } catch (err) {
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

  <section class="settings-section">
    <h3>AI</h3>

    <form onsubmit={handleAiTransportSubmit}>
      <label for="ai-provider">Provider</label>
      <div class="row">
        <select
          id="ai-provider"
          bind:value={aiProviderKind}
          onchange={handleAiProviderChange}
        >
          <option value="none">None</option>
          <option value="openAiCompatible">OpenAI-compatible</option>
          <option value="anthropic">Anthropic</option>
        </select>
      </div>

      {#if aiProviderKind !== "none"}
        <label for="ai-base-url">Base URL</label>
        <div class="row">
          <input
            id="ai-base-url"
            type="text"
            placeholder="http://localhost:11434/v1"
            bind:value={aiBaseUrl}
            oninput={handleAiTransportInput}
          />
        </div>

        <label for="ai-model">Model</label>
        <div class="row">
          <input
            id="ai-model"
            type="text"
            placeholder="llama3.1"
            bind:value={aiModel}
            oninput={handleAiTransportInput}
          />
        </div>
      {/if}

      <div class="row">
        <button type="submit">Save</button>
      </div>
      <p class="hint">
        {aiTransportSaved
          ? "Saved."
          : "Nothing is sent anywhere until you set a provider here and click \"Generate with AI\" in the commit box."}
      </p>
    </form>

    <form onsubmit={handleAiApiKeySubmit}>
      <label for="ai-api-key">API key (optional for local servers)</label>
      <div class="row">
        <input
          id="ai-api-key"
          type="password"
          placeholder={settingsState.hasAiApiKey ? "Key saved — enter a new value to replace it" : "No key saved"}
          bind:value={aiApiKeyDraft}
          oninput={handleAiApiKeyInput}
        />
        <button type="submit" disabled={!aiApiKeyDraft.trim()}>Save</button>
        {#if settingsState.hasAiApiKey}
          <button type="button" onclick={handleAiApiKeyClear}>Clear</button>
        {/if}
      </div>
      <p class="hint">
        {aiApiKeySaved
          ? "Saved."
          : "Stored in your OS keyring, never in this app's plain-text config file."}
      </p>
    </form>

    <form onsubmit={handleAiInstructionsSubmit}>
      <label for="ai-instructions">Custom instructions</label>
      <textarea
        id="ai-instructions"
        rows="3"
        placeholder={`e.g. "use Conventional Commits", "keep the summary under 50 characters"`}
        bind:value={aiInstructions}
        oninput={handleAiInstructionsInput}
      ></textarea>
      <div class="row">
        <button type="submit">Save</button>
      </div>
      <p class="hint">
        {aiInstructionsSaved ? "Saved." : "Appended to every generation prompt."}
      </p>
    </form>
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

  .row input,
  .row select,
  textarea {
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

  textarea {
    resize: none;
  }

  .row input:focus-visible,
  .row select:focus-visible,
  textarea:focus-visible {
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

  .row button:hover:not(:disabled) {
    background: var(--surface-2);
  }

  .row button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
</style>
