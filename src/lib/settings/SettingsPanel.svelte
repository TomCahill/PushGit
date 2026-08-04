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
    cancelLocalAiDownload,
    clearAiApiKey,
    downloadLocalAi,
    fetchLocalAiStatus,
    setAiApiKey,
    setAiInstructions,
    setAiTransport,
    setAutoFetchEnabled,
    setAutoFetchIntervalMinutes,
    setMaxCommitsRendered,
    setReduceMotion,
    settingsState,
  } from "./settings.svelte";
  import { notifyError } from "$lib/shell/toast.svelte";
  import Button from "$lib/shell/Button.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import Select from "$lib/shell/Select.svelte";
  import Switch from "$lib/shell/Switch.svelte";
  import TextField from "$lib/shell/TextField.svelte";
  import type { AiTransport, EngineVariant, LocalAiStatus, WorkflowConfig } from "$lib/git/types";

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

  async function handleAutoFetchEnabledChange() {
    const value = settingsState.autoFetchEnabled;
    try {
      await setAutoFetchEnabled(value);
    } catch (err) {
      settingsState.autoFetchEnabled = !value; // revert the optimistic checkbox toggle
      notifyError(String(err));
    }
  }

  // Draft, not bound straight to `settingsState`, since `Select`'s value is a string and the
  // backend field is a number — same "local draft, save on change" shape as `aiEngineVariant`.
  let autoFetchIntervalDraft = $state(String(settingsState.autoFetchIntervalMinutes));

  async function handleAutoFetchIntervalChange() {
    try {
      await setAutoFetchIntervalMinutes(Number(autoFetchIntervalDraft));
    } catch (err) {
      notifyError(String(err));
    } finally {
      autoFetchIntervalDraft = String(settingsState.autoFetchIntervalMinutes);
    }
  }

  // AI provider/model/instructions/key — the single place any of this feature's
  // configuration is surfaced (the commit box's "Generate with AI" button carries none of
  // it). `aiProviderKind`/`aiBaseUrl`/`aiModel` are drafted from `settingsState.ai.transport`
  // once at mount, the same "local draft + explicit Save" pattern as the commit-template
  // field above, rather than two-way binding straight to shared state like the plain
  // checkboxes do — a provider/URL/model change should only take effect together, on submit.
  type AiProviderKind = "none" | AiTransport["kind"];
  const initialTransport = settingsState.ai.transport;
  let aiProviderKind = $state<AiProviderKind>(initialTransport?.kind ?? "none");
  let aiBaseUrl = $state(initialTransport && "baseUrl" in initialTransport ? initialTransport.baseUrl : "");
  let aiModel = $state(initialTransport && "model" in initialTransport ? initialTransport.model : "");
  // Defaults to "cpu" the first time a user selects "Local AI" — guaranteed to work everywhere,
  // never auto-picked into Vulkan on their behalf.
  let aiEngineVariant = $state<EngineVariant>(
    initialTransport?.kind === "managedLocal" ? initialTransport.engineVariant : "cpu",
  );
  let aiTransportSaved = $state(false);

  // Local-AI status for the *drafted* engine variant, previewed independently of the saved
  // transport (`settingsState.localAiStatus`) — switching CPU/GPU in this form updates its own
  // status line without requiring Save first, per the feature plan's "operate on the draft, not
  // last-saved state" rule (the same pattern the base-URL/model fields already follow).
  let draftLocalAiStatus = $state<LocalAiStatus>({
    modelPresent: false,
    enginePresent: false,
    gpuDevice: null,
  });

  async function refreshDraftLocalAiStatus(variant: EngineVariant) {
    try {
      draftLocalAiStatus = await fetchLocalAiStatus(variant);
    } catch {
      // Leave the previous value; this is a preview convenience only.
    }
  }

  $effect(() => {
    if (aiProviderKind === "managedLocal") {
      void refreshDraftLocalAiStatus(aiEngineVariant);
    }
  });

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
    let transport: AiTransport | null;
    if (aiProviderKind === "none") {
      transport = null;
    } else if (aiProviderKind === "managedLocal") {
      transport = { kind: "managedLocal", engineVariant: aiEngineVariant };
    } else {
      transport = { kind: aiProviderKind, baseUrl: aiBaseUrl.trim(), model: aiModel.trim() };
    }
    try {
      await setAiTransport(transport);
      aiTransportSaved = true;
    } catch (err) {
      notifyError(String(err));
    }
  }

  // Local-AI download — a direct click starts it; no confirmation dialog, since this pulls
  // bytes onto the machine rather than sending anything off it (the cloud-egress warning
  // elsewhere in this panel is about the opposite direction).
  const CANCELLED_PATTERN = /operation cancelled/i;

  // Tracked separately from `settingsState.localAiDownloadProgress`: a moment can pass after
  // the click (DNS/connect) before the first progress chunk arrives, and the Cancel button
  // needs to be there for that whole window, not just once progress data exists.
  let downloadingLocalAi = $state(false);

  async function handleDownloadLocalAi() {
    downloadingLocalAi = true;
    try {
      await downloadLocalAi(aiEngineVariant);
    } catch (err) {
      if (!CANCELLED_PATTERN.test(String(err))) {
        notifyError(String(err));
      }
    } finally {
      downloadingLocalAi = false;
      void refreshDraftLocalAiStatus(aiEngineVariant);
    }
  }

  function handleAiEngineVariantChange() {
    aiTransportSaved = false;
  }

  function handleCancelLocalAiDownload() {
    void cancelLocalAiDownload();
  }

  function downloadPercent(bytesDownloaded: number, bytesTotal: number): number {
    return bytesTotal > 0 ? Math.round((bytesDownloaded / bytesTotal) * 100) : 0;
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
  <div class="settings-content">
    <section class="settings-card">
      <div class="card-header">
        <Icon name="settings" size={16} />
        <h3>Application</h3>
      </div>

      <form onsubmit={handleSubmit}>
        <TextField
          id="max-commits-rendered"
          label="Max commits rendered in graph"
          inputmode="numeric"
          bind:value={draftValue}
          oninput={handleInput}
          hint={saved ? "Saved." : `A new page of commits loads at most this many rows at a time.`}
        />
        <div class="row">
          <Button variant="tonal" type="submit">Save</Button>
        </div>
      </form>

      <label class="switch-row">
        <div class="switch-row-text">
          <span>Reduce motion</span>
        </div>
        <Switch bind:checked={settingsState.reduceMotion} onchange={handleReduceMotionChange} />
      </label>

      <label class="switch-row">
        <div class="switch-row-text">
          <span>Auto-fetch</span>
        </div>
        <Switch
          bind:checked={settingsState.autoFetchEnabled}
          onchange={handleAutoFetchEnabledChange}
        />
      </label>
      <p class="hint">
        Periodically fetches the current repo's "origin" in the background — the same remote a
        manual Fetch click would use.
      </p>

      {#if settingsState.autoFetchEnabled}
        <Select
          id="auto-fetch-interval"
          label="Auto-fetch interval"
          bind:value={autoFetchIntervalDraft}
          onchange={handleAutoFetchIntervalChange}
        >
          <option value="5">Every 5 minutes</option>
          <option value="10">Every 10 minutes</option>
          <option value="15">Every 15 minutes</option>
          <option value="30">Every 30 minutes</option>
          <option value="60">Every 60 minutes</option>
        </Select>
      {/if}
    </section>

    <section class="settings-card">
      <div class="card-header">
        <Icon name="sparkles" size={16} />
        <h3>AI</h3>
      </div>

      <form onsubmit={handleAiTransportSubmit}>
        <div class="ai-transport-fields">
          <Select
            id="ai-provider"
            label="Provider"
            bind:value={aiProviderKind}
            onchange={handleAiProviderChange}
          >
            <option value="none">None</option>
            <option value="openAiCompatible">OpenAI-compatible</option>
            <option value="anthropic">Anthropic</option>
            <option value="managedLocal">Local AI</option>
          </Select>

          {#if aiProviderKind === "openAiCompatible" || aiProviderKind === "anthropic"}
            <TextField
              id="ai-base-url"
              label="Base URL"
              placeholder="http://localhost:11434/v1"
              bind:value={aiBaseUrl}
              oninput={handleAiTransportInput}
            />

            <TextField
              id="ai-model"
              label="Model"
              placeholder="llama3.1"
              bind:value={aiModel}
              oninput={handleAiTransportInput}
            />
          {:else if aiProviderKind === "managedLocal"}
            <div class="local-ai-status">
              <Select
                id="ai-engine-variant"
                label="Engine"
                bind:value={aiEngineVariant}
                onchange={handleAiEngineVariantChange}
              >
                <option value="cpu">CPU</option>
                <option value="vulkan">GPU (Vulkan)</option>
              </Select>
              {#if aiEngineVariant === "vulkan"}
                <p class="hint">
                  Requires a working Vulkan driver — most desktop Linux installs with a GPU already
                  have one. Falls back to no speedup, not an error, if none is found.
                </p>
              {/if}

              {#if downloadingLocalAi}
                {#if settingsState.localAiDownloadProgress}
                  {@const p = settingsState.localAiDownloadProgress}
                  <div class="progress" role="status">
                    <div class="progress-track">
                      <div
                        class="progress-fill"
                        style={`width: ${downloadPercent(p.bytesDownloaded, p.bytesTotal)}%`}
                      ></div>
                    </div>
                    <p class="progress-label">
                      {p.stage === "engine" ? "Engine" : "Model"}: {downloadPercent(
                        p.bytesDownloaded,
                        p.bytesTotal,
                      )}%
                    </p>
                  </div>
                {:else}
                  <p class="hint">Starting download…</p>
                {/if}
                <div class="row">
                  <Button variant="outlined" type="button" onclick={handleCancelLocalAiDownload}>
                    Cancel
                  </Button>
                </div>
              {:else if draftLocalAiStatus.modelPresent && draftLocalAiStatus.enginePresent}
                <p class="hint">Ready — Qwen2.5-Coder-1.5B running locally via llama.cpp.</p>
                {#if aiEngineVariant === "vulkan"}
                  {#if draftLocalAiStatus.gpuDevice}
                    <p class="hint">GPU: {draftLocalAiStatus.gpuDevice}</p>
                  {:else}
                    <p class="hint">No Vulkan-capable GPU detected — this will run on CPU.</p>
                  {/if}
                {/if}
              {:else}
                <p class="hint">Not downloaded yet (~1.1GB).</p>
                <div class="row">
                  <Button variant="tonal" type="button" onclick={handleDownloadLocalAi}>
                    <Icon name="download" size={13} /> Download Local AI Engine
                  </Button>
                </div>
              {/if}
            </div>
          {/if}
        </div>

        <div class="row">
          <Button variant="tonal" type="submit">Save</Button>
        </div>
        <p class="hint">
          {aiTransportSaved
            ? "Saved."
            : 'Nothing is sent anywhere until you set a provider here and click "Generate with AI" in the commit box.'}
        </p>
      </form>

      <form onsubmit={handleAiApiKeySubmit}>
        <TextField
          id="ai-api-key"
          label="API key (optional for local servers)"
          type="password"
          placeholder={settingsState.hasAiApiKey ? "Key saved — enter a new value to replace it" : "No key saved"}
          bind:value={aiApiKeyDraft}
          oninput={handleAiApiKeyInput}
        />
        <div class="row">
          <Button variant="tonal" type="submit" disabled={!aiApiKeyDraft.trim()}>Save</Button>
          {#if settingsState.hasAiApiKey}
            <Button variant="outlined" type="button" onclick={handleAiApiKeyClear}>Clear</Button>
          {/if}
        </div>
        <p class="hint">
          {aiApiKeySaved ? "Saved." : "Stored in your OS keyring, never in this app's plain-text config file."}
        </p>
      </form>

      <form onsubmit={handleAiInstructionsSubmit}>
        <TextField
          id="ai-instructions"
          label="Custom instructions"
          multiline
          placeholder={`e.g. "use Conventional Commits", "keep the summary under 50 characters"`}
          bind:value={aiInstructions}
          oninput={handleAiInstructionsInput}
          hint={aiInstructionsSaved ? "Saved." : "Appended to every generation prompt."}
        />
        <div class="row">
          <Button variant="tonal" type="submit">Save</Button>
        </div>
      </form>
    </section>

    {#if repoPath}
      <section class="settings-card">
        <div class="card-header">
          <Icon name="wrench" size={16} />
          <h3>This Repository</h3>
        </div>

        <label class="switch-row">
          <div class="switch-row-text">
            <span>Skip hooks by default</span>
          </div>
          <Switch bind:checked={defaultSkipHooks} onchange={handleSkipHooksChange} />
        </label>

        <form onsubmit={handleTemplateSubmit}>
          <TextField
            id="commit-template-path"
            label="Commit message template"
            placeholder="No template configured"
            bind:value={commitTemplatePath}
            oninput={handleTemplateInput}
            hint={repoSaved
              ? "Saved."
              : "Sets this repo's git commit.template path — clear the field to unset it."}
          />
          <div class="row">
            <Button variant="tonal" type="submit">Save</Button>
          </div>
        </form>

        {#if workflowConfig}
          <div class="workflow-status">
            <span class="workflow-label">GitFlow</span>
            <p class="hint">
              Configured — main branch "{workflowConfig.main}", develop branch "{workflowConfig.develop}".
              Start/finish feature, release, and hotfix branches from the Workflow panel.
            </p>
          </div>
        {:else}
          <form class="workflow-setup" onsubmit={handleWorkflowSubmit}>
            <span class="workflow-label">Set up GitFlow</span>
            <div class="row">
              <TextField bind:value={workflowMain} placeholder="main" ariaLabel="Main branch name" />
              <TextField
                bind:value={workflowDevelop}
                placeholder="develop"
                ariaLabel="Develop branch name"
              />
            </div>
            <div class="row">
              <TextField
                bind:value={workflowFeaturePrefix}
                placeholder="feature/"
                ariaLabel="Feature branch prefix"
              />
              <TextField
                bind:value={workflowReleasePrefix}
                placeholder="release/"
                ariaLabel="Release branch prefix"
              />
              <TextField
                bind:value={workflowHotfixPrefix}
                placeholder="hotfix/"
                ariaLabel="Hotfix branch prefix"
              />
            </div>
            <div class="row">
              <TextField
                bind:value={workflowVersionTagPrefix}
                placeholder="Version tag prefix (blank for none)"
                ariaLabel="Version tag prefix"
              />
              <Button variant="tonal" type="submit">Set up</Button>
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
</div>

<style>
  .settings-panel {
    display: flex;
    justify-content: center;
    min-width: 14rem;
  }

  .settings-content {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    width: 100%;
    max-width: 40rem;
    padding-bottom: var(--space-4);
  }

  .settings-card {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    background: var(--surface-1);
    border-radius: var(--radius-lg);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--accent);
  }

  .card-header h3 {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .ai-transport-fields {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding-left: var(--space-3);
    border-left: 2px solid var(--border);
  }

  .local-ai-status {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .progress {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .progress-track {
    flex: 1 1 auto;
    height: 0.3rem;
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s ease;
  }

  .progress-label {
    flex-shrink: 0;
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .workflow-status,
  .workflow-setup {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .workflow-label {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .switch-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    font-size: 0.85rem;
    color: var(--text-primary);
    cursor: pointer;
  }

  .switch-row-text {
    display: flex;
    flex-direction: column;
  }

  .row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .hint {
    margin: 0;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
</style>
