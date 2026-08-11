<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Working-directory staging UI: unstaged/staged file lists, per-hunk and per-line
  // stage/unstage, and the commit box.
  import {
    cancelAiGeneration,
    commitChanges,
    commitMessageTemplate,
    createStashForPaths,
    diffStaged,
    diffUnstaged,
    discardFileChanges,
    generateCommitMessage,
    getRepoConfig,
    headCommitMessage,
    stageFile,
    stageHunk,
    stageLines,
    unstageFile,
    unstageHunk,
    unstageLines,
  } from "$lib/git/api";
  import DiffStat from "$lib/diff/DiffStat.svelte";
  import FileStatusIcon from "$lib/diff/FileStatusIcon.svelte";
  import { confirmAsync } from "$lib/shell/confirmDialog.svelte";
  import Button from "$lib/shell/Button.svelte";
  import { openContextMenu, type ContextMenuItem } from "$lib/shell/contextMenu.svelte";
  import { finishHookOutput, pushHookOutputLine, startHookOutput } from "$lib/shell/hookOutput.svelte";
  import Icon from "$lib/shell/Icon.svelte";
  import ResizeHandle from "$lib/shell/ResizeHandle.svelte";
  import { acknowledgeAiCloudWarning, settingsState } from "$lib/settings/settings.svelte";
  import { sectionHeightsState, setStagedHeight, setUnstagedHeight } from "./sectionHeights.svelte";
  import type { AiTransport, FileDiff, FileDiffSelection, Hunk } from "$lib/git/types";

  let {
    repoPath,
    refreshKey,
    onChanged,
    onResolveConflict,
    onBlame,
    selectedFile = $bindable(null),
    onDiffChange,
  }: {
    repoPath: string;
    refreshKey: number;
    /** Fires after a commit or a discard — anything the rest of the shell (branch/graph/undo
     *  status) needs to refresh for. */
    onChanged?: () => void;
    onResolveConflict?: (path: string) => void;
    onBlame?: (path: string) => void;
    /** Which file's diff is selected — bindable so the shell can clear it (e.g. a "back to
     *  graph" control) as well as read it. */
    selectedFile?: { path: string; staged: boolean } | null;
    /** Fires whenever the selected file's diff (or its stage/unstage actions) changes, so the
     *  shell can render `HunkDiff` for it in the center panel — this panel no longer renders
     *  the diff itself. */
    onDiffChange?: (diff: FileDiffSelection | null) => void;
  } = $props();

  let unstagedFiles = $state<FileDiff[]>([]);
  let stagedFiles = $state<FileDiff[]>([]);
  let loadError = $state<string | null>(null);

  let title = $state("");
  let description = $state("");
  let amend = $state(false);
  let skipHooks = $state(false);
  // This repo's "skip hooks by default" setting
  // (`SettingsPanel.svelte`'s "This Repository" section) — `skipHooks` above is initialized
  // from it on repo load/switch, and falls back to it again after each commit, rather than
  // always resetting to unchecked. `skipHooksTouched` guards the load against clobbering a
  // manual check that lands before it resolves, same concern `prefillFromTemplate` handles
  // for the commit-template prefill below.
  let skipHooksDefault = false;
  let skipHooksTouched = false;
  let committing = $state(false);
  let commitError = $state<string | null>(null);

  let generating = $state(false);
  let aiError = $state<string | null>(null);
  // Bumped on cancel/repo-switch so a chunk from an abandoned generation can never write into
  // fields that now belong to a different repo (or a fresh generation) — same
  // stale-response-guard idea `reload`'s `generation` counter already uses below.
  let aiGeneration = 0;
  // Set by a real keystroke (not our own programmatic chunk writes — see the title/
  // description inputs' `oninput` below) while a generation is in flight, so a manual edit
  // mid-stream stops chunks from continuing to overwrite it.
  let aiUserEditedDuringGeneration = false;
  // Matches `PushGitError::Cancelled`'s `#[error("operation cancelled")]` message
  // (`RemotePanel.svelte` uses this same pattern for fetch/pull/push) — a user-initiated stop
  // isn't a failure, so it's cleared quietly rather than shown as an error.
  const CANCELLED_PATTERN = /operation cancelled/i;

  const TITLE_MAX_LENGTH = 72;

  // Some models (small local ones especially — weaker instruction-following than the cloud
  // models this prompt was first tuned against) wrap their output in a markdown code fence
  // despite the system prompt explicitly forbidding it. Stripped defensively here rather than
  // trusted away, since a leading ```lang line would otherwise become the commit title
  // verbatim. Applied on every chunk against the whole accumulated buffer (not incrementally),
  // so a fence still being typed out mid-stream may render partially stripped for a moment —
  // harmless, since it settles to the correct result once the opening line finishes streaming.
  function stripCodeFence(text: string): string {
    return text.replace(/^```[^\n]*\n?/, "").replace(/\n?```\s*$/, "");
  }

  function splitMessage(full: string): { title: string; description: string } {
    const stripped = stripCodeFence(full);
    const newlineIndex = stripped.indexOf("\n");
    if (newlineIndex === -1) return { title: stripped, description: "" };
    return {
      title: stripped.slice(0, newlineIndex),
      description: stripped.slice(newlineIndex + 1).replace(/^\n+/, ""),
    };
  }

  // The managed local model can't be trusted to stop itself at a bullet count — verified
  // directly against the real model that asking it to cap at 3-4 bullets either gets ignored
  // (it enumerates a bullet per file regardless) or, worded more mechanically, degenerates into
  // repeating the same line until the token limit. Enforced here instead: once a bullet beyond
  // `MAX_LOCAL_BULLETS` starts streaming in, the displayed text freezes right before it and
  // `handleGenerateWithAi` cancels the underlying generation — this also happens to catch the
  // repetition-loop failure mode, since a 5th bullet-shaped line triggers the cap regardless of
  // whether the first four were sensible or just the same line repeated.
  const MAX_LOCAL_BULLETS = 4;

  function capBullets(
    description: string,
    maxBullets: number,
  ): { capped: string; exceeded: boolean } {
    const lines = description.split("\n");
    let bulletCount = 0;
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].startsWith("- ")) {
        bulletCount++;
        if (bulletCount > maxBullets) {
          return { capped: lines.slice(0, i).join("\n").replace(/\n+$/, ""), exceeded: true };
        }
      }
    }
    return { capped: description, exceeded: false };
  }

  function buildMessage(titleText: string, descriptionText: string): string {
    const trimmedDescription = descriptionText.trim();
    return trimmedDescription ? `${titleText}\n\n${trimmedDescription}` : titleText;
  }

  // Mirrors real `git commit`'s own `commit.template` pre-fill, without ever clobbering text
  // the user already typed (or that `handleAmendToggle` already filled in) — only applies when
  // both fields are still blank. Called once per repo open/switch (the effect below) and again
  // after each successful non-amend commit, since real git re-applies the template to every new
  // commit's editor, not just the first one in a session.
  //
  // The "still blank" check deliberately happens *after* the `await`, not before: an `$effect`
  // tracks every reactive read in its synchronous call graph, including inside a called async
  // function up to its first `await` — reading `title`/`description` there would make this
  // function's caller-effect (below) re-fire every time `title`/`description` change, which
  // includes `handleCommit` clearing them back to "" right before it calls this again itself.
  async function prefillFromTemplate(path: string) {
    if (!path) return;
    let template: string | null;
    try {
      template = await commitMessageTemplate(path);
    } catch {
      return; // No commit.template configured, or its file is unreadable.
    }
    if (!template || title !== "" || description !== "") return;
    const split = splitMessage(template);
    title = split.title;
    description = split.description;
  }

  async function loadSkipHooksDefault(path: string) {
    if (!path) return;
    try {
      const config = await getRepoConfig(path);
      if (skipHooksTouched) return;
      skipHooksDefault = config.defaultSkipHooks;
      skipHooks = skipHooksDefault;
    } catch {
      // Leave skipHooks/skipHooksDefault at their current values.
    }
  }

  function handleSkipHooksChange() {
    skipHooksTouched = true;
  }

  // A half-configured transport (provider picked but base URL/model left blank) stays
  // disabled rather than lighting up and failing at request time — pure frontend check
  // against the already-loaded `AiSettings`, no extra backend call.
  const aiDisabledReason = $derived.by(() => {
    if (stagedFiles.length === 0) return "Stage changes first";
    const transport = settingsState.ai.transport;
    if (!transport) return "Configure an AI provider in Settings first";
    if (transport.kind === "managedLocal") {
      const ready =
        settingsState.localAiStatus.modelPresent && settingsState.localAiStatus.enginePresent;
      return ready ? null : "Download the local AI engine in Settings first";
    }
    const configured = transport.baseUrl.trim() !== "" && transport.model.trim() !== "";
    return configured ? null : "Configure an AI provider in Settings first";
  });

  const generateAiTooltip = $derived(
    generating ? "Stop generating" : (aiDisabledReason ?? "Generate a commit message from the staged diff"),
  );

  function isLocalTransport(transport: AiTransport): boolean {
    if (transport.kind === "managedLocal") return true; // always loopback — no cloud-egress warning
    try {
      const host = new URL(transport.baseUrl).hostname;
      return host === "localhost" || host === "127.0.0.1";
    } catch {
      return false; // an unparseable base URL doesn't get treated as local
    }
  }

  function handleTitleInputWhileGenerating() {
    if (generating) aiUserEditedDuringGeneration = true;
  }

  function handleDescriptionInputWhileGenerating() {
    if (generating) aiUserEditedDuringGeneration = true;
  }

  async function handleGenerateWithAi() {
    if (generating || aiDisabledReason) return;
    const transport = settingsState.ai.transport;
    if (!transport) return;

    if (
      transport.kind !== "managedLocal" &&
      !isLocalTransport(transport) &&
      !settingsState.ai.cloudWarningAcknowledged
    ) {
      const confirmed = await confirmAsync(
        `This will send your staged diff to ${transport.baseUrl}. Staged changes can include secrets — review what's staged before continuing.`,
      );
      if (!confirmed) return;
      try {
        await acknowledgeAiCloudWarning();
      } catch (err) {
        aiError = String(err);
        return;
      }
    }

    if (title !== "" || description !== "") {
      const confirmed = await confirmAsync(
        "Replace the current commit title/description with an AI-generated one?",
      );
      if (!confirmed) return;
    }

    const myGeneration = ++aiGeneration;
    title = "";
    description = "";
    aiError = null;
    aiUserEditedDuringGeneration = false;
    generating = true;
    let buffer = "";
    const isManagedLocal = transport.kind === "managedLocal";
    let bulletCapReached = false;
    try {
      await generateCommitMessage(repoPath, (chunk) => {
        if (myGeneration !== aiGeneration || aiUserEditedDuringGeneration || bulletCapReached) {
          return;
        }
        buffer += chunk;
        const split = splitMessage(buffer);
        title = split.title;
        if (isManagedLocal) {
          const { capped, exceeded } = capBullets(split.description, MAX_LOCAL_BULLETS);
          description = capped;
          if (exceeded) {
            bulletCapReached = true;
            void cancelAiGeneration(repoPath);
          }
        } else {
          description = split.description;
        }
      });
    } catch (err) {
      if (myGeneration === aiGeneration) {
        const message = String(err);
        if (!CANCELLED_PATTERN.test(message)) aiError = message;
      }
    } finally {
      if (myGeneration === aiGeneration) generating = false;
    }
  }

  function handleStopGeneration() {
    void cancelAiGeneration(repoPath);
  }

  let generation = 0;

  $effect(() => {
    void reload(repoPath, refreshKey);
  });

  $effect(() => {
    void prefillFromTemplate(repoPath);
  });

  $effect(() => {
    skipHooksTouched = false;
    void loadSkipHooksDefault(repoPath);
  });

  // Cancels any in-flight AI generation the moment `repoPath` changes (or this panel
  // unmounts), the same shape as the effect above that resets `skipHooksTouched` on repo
  // change — a stream from the old repo can never keep writing into the new one's fields.
  // Whatever partial text already streamed in is left as-is, same as if the user had typed it.
  $effect(() => {
    const path = repoPath;
    return () => {
      if (generating) {
        aiGeneration++;
        generating = false;
        void cancelAiGeneration(path);
      }
    };
  });

  function fileKey(file: FileDiff): string {
    return file.newPath ?? file.oldPath ?? "";
  }

  function fileLabel(file: FileDiff): string {
    if (
      (file.status === "renamed" || file.status === "copied") &&
      file.oldPath &&
      file.newPath &&
      file.oldPath !== file.newPath
    ) {
      return `${file.oldPath} → ${file.newPath}`;
    }
    return fileKey(file);
  }

  function sumStats(files: FileDiff[]): { insertions: number; deletions: number } {
    return files.reduce(
      (acc, f) => ({
        insertions: acc.insertions + f.insertions,
        deletions: acc.deletions + f.deletions,
      }),
      { insertions: 0, deletions: 0 },
    );
  }

  const stagedStats = $derived(sumStats(stagedFiles));
  const unstagedStats = $derived(sumStats(unstagedFiles));
  const stageableFiles = $derived(unstagedFiles.filter((f) => f.status !== "conflicted"));

  async function reload(path: string, _refreshKey: number) {
    const myGeneration = ++generation;
    if (!path) {
      unstagedFiles = [];
      stagedFiles = [];
      selectedFile = null;
      return;
    }

    loadError = null;
    try {
      const [unstaged, staged] = await Promise.all([diffUnstaged(path), diffStaged(path)]);
      if (myGeneration !== generation) return;
      unstagedFiles = unstaged;
      stagedFiles = staged;
      if (selectedFile) {
        const list = selectedFile.staged ? stagedFiles : unstagedFiles;
        if (!list.some((f) => fileKey(f) === selectedFile!.path)) selectedFile = null;
      }
    } catch (err) {
      if (myGeneration === generation) loadError = String(err);
    }
  }

  const selectedDiff = $derived(
    selectedFile
      ? ((selectedFile.staged ? stagedFiles : unstagedFiles).find(
          (f) => fileKey(f) === selectedFile!.path,
        ) ?? null)
      : null,
  );

  // Reports the selected file's diff (and its stage/unstage actions) up to the shell, which
  // renders `HunkDiff` for it in the center panel — see `onDiffChange` above.
  $effect(() => {
    const file = selectedDiff;
    const sel = selectedFile;
    if (!file || !sel) {
      onDiffChange?.(null);
      return;
    }
    onDiffChange?.({
      file,
      hunkActionLabel: sel.staged ? "Unstage hunk" : "Stage hunk",
      onHunkAction: (hunk: Hunk) => handleHunkToggle(sel.path, sel.staged, hunk),
      lineActionLabel: sel.staged ? "Unstage" : "Stage",
      onLineAction: (hunk: Hunk, lineIndices: number[]) =>
        handleLinesToggle(sel.path, sel.staged, hunk, lineIndices),
    });
  });

  function selectFile(file: FileDiff, staged: boolean) {
    selectedFile = { path: fileKey(file), staged };
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard access can be denied — a silent no-op beats an error banner for a purely
      // cosmetic convenience action, matching `CopyButton.svelte`'s own handling.
    }
  }

  function buildUnstagedMenu(file: FileDiff): ContextMenuItem[] {
    const path = fileKey(file);
    const items: ContextMenuItem[] = [
      { label: "Copy file path", onSelect: () => void copyText(path) },
    ];
    if (onBlame) items.push({ label: "Blame", onSelect: () => onBlame?.(path) });
    if (file.status === "conflicted") return items;
    items.push({ separator: true });
    items.push({ label: "Stash", onSelect: () => void handleStashFile(file) });
    items.push({ label: "Discard changes", danger: true, onSelect: () => void handleDiscardFile(file) });
    return items;
  }

  function handleUnstagedContextMenu(event: MouseEvent, file: FileDiff) {
    event.preventDefault();
    selectFile(file, false);
    openContextMenu(event.clientX, event.clientY, buildUnstagedMenu(file));
  }

  function buildStagedMenu(file: FileDiff): ContextMenuItem[] {
    return [{ label: "Copy file path", onSelect: () => void copyText(fileKey(file)) }];
  }

  function handleStagedContextMenu(event: MouseEvent, file: FileDiff) {
    event.preventDefault();
    selectFile(file, true);
    openContextMenu(event.clientX, event.clientY, buildStagedMenu(file));
  }

  async function handleStageFile(file: FileDiff, event: MouseEvent) {
    event.stopPropagation();
    try {
      await stageFile(repoPath, fileKey(file));
      await reload(repoPath, refreshKey);
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleStageAll() {
    try {
      for (const file of stageableFiles) {
        await stageFile(repoPath, fileKey(file));
      }
      await reload(repoPath, refreshKey);
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleDiscardFile(file: FileDiff) {
    const path = fileKey(file);
    if (
      !(await confirmAsync(
        `Discard changes to "${path}"? This can be undone from the toolbar's Undo button.`,
      ))
    ) {
      return;
    }
    try {
      await discardFileChanges(repoPath, path);
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleStashFile(file: FileDiff) {
    const path = fileKey(file);
    try {
      await createStashForPaths(repoPath, [path]);
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleUnstageFile(file: FileDiff, event: MouseEvent) {
    event.stopPropagation();
    try {
      await unstageFile(repoPath, fileKey(file));
      await reload(repoPath, refreshKey);
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleHunkToggle(path: string, staged: boolean, hunk: Hunk) {
    try {
      if (staged) {
        await unstageHunk(repoPath, path, hunk);
      } else {
        await stageHunk(repoPath, path, hunk);
      }
      await reload(repoPath, refreshKey);
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleLinesToggle(
    path: string,
    staged: boolean,
    hunk: Hunk,
    lineIndices: number[],
  ) {
    try {
      if (staged) {
        await unstageLines(repoPath, path, hunk, lineIndices);
      } else {
        await stageLines(repoPath, path, hunk, lineIndices);
      }
      await reload(repoPath, refreshKey);
    } catch (err) {
      loadError = String(err);
    }
  }

  async function handleAmendToggle(event: Event) {
    // Captured synchronously — `event.currentTarget` is nulled out once dispatch finishes,
    // so it's unusable after the `await confirmAsync` below.
    const checkbox = event.currentTarget as HTMLInputElement;
    const turningOn = !amend;
    if (turningOn && (title !== "" || description !== "")) {
      const confirmed = await confirmAsync(
        "Replace the current commit title/description with the last commit's message?",
      );
      if (!confirmed) {
        // The click already flipped the native checkbox before this async confirm resolved —
        // revert it to match `amend`, which we're about to leave unchanged.
        checkbox.checked = amend;
        return;
      }
    }
    amend = turningOn;
    if (!amend) return;
    try {
      const split = splitMessage((await headCommitMessage(repoPath)) ?? "");
      title = split.title;
      description = split.description;
    } catch (err) {
      commitError = String(err);
    }
  }

  function handleFormKeydown(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      (event.currentTarget as HTMLFormElement).requestSubmit();
    }
  }

  const commitDisabledReason = $derived.by(() => {
    if (!title.trim()) return "Enter a commit title";
    if (!amend && stagedFiles.length === 0) return "Stage changes first";
    return null;
  });

  async function handleCommit(event: SubmitEvent) {
    event.preventDefault();
    if (committing || commitDisabledReason !== null) return;
    committing = true;
    commitError = null;
    startHookOutput(amend ? "Amend" : "Commit");
    try {
      await commitChanges(
        repoPath,
        buildMessage(title, description),
        amend,
        skipHooks,
        pushHookOutputLine,
      );
      title = "";
      description = "";
      amend = false;
      skipHooks = skipHooksDefault;
      skipHooksTouched = false;
      selectedFile = null;
      void prefillFromTemplate(repoPath);
      await reload(repoPath, refreshKey);
      onChanged?.();
    } catch (err) {
      commitError = String(err);
    } finally {
      committing = false;
      finishHookOutput();
    }
  }
</script>

<div class="staging">
  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {/if}

  <div class="scroll-sections">
    <section
      class="file-group"
      aria-label="Unstaged changes"
      style:height="{sectionHeightsState.unstaged}px"
    >
      <h3>
        Changes ({unstagedFiles.length})
        <DiffStat insertions={unstagedStats.insertions} deletions={unstagedStats.deletions} />
        <button
          type="button"
          class="stage-all"
          disabled={stageableFiles.length === 0}
          onclick={handleStageAll}
        >
          Stage All
        </button>
      </h3>
      <ul>
        {#each unstagedFiles as file (fileKey(file))}
          <li
            class:selected={selectedFile?.staged === false && selectedFile.path === fileKey(file)}
            oncontextmenu={(event) => handleUnstagedContextMenu(event, file)}
          >
            <button type="button" class="file-row" onclick={() => selectFile(file, false)}>
              <FileStatusIcon status={file.status} />
              <span class="path">{fileLabel(file)}</span>
              <DiffStat insertions={file.insertions} deletions={file.deletions} />
            </button>
            <div class="file-actions">
              {#if file.status === "conflicted" && onResolveConflict}
                <button
                  type="button"
                  class="resolve-conflict"
                  title="Resolve conflict"
                  onclick={(event) => {
                    event.stopPropagation();
                    onResolveConflict?.(fileKey(file));
                  }}
                >
                  Resolve
                </button>
              {:else}
                <button
                  type="button"
                  class="stage-toggle"
                  title="Stage"
                  onclick={(event) => handleStageFile(file, event)}
                >
                  +
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>

    <ResizeHandle
      orientation="horizontal"
      onResize={(dy) => setUnstagedHeight(sectionHeightsState.unstaged + dy)}
    />

    <section
      class="file-group"
      aria-label="Staged changes"
      style:height="{sectionHeightsState.staged}px"
    >
      <h3>
        Staged Changes ({stagedFiles.length})
        <DiffStat insertions={stagedStats.insertions} deletions={stagedStats.deletions} />
      </h3>
      <ul>
        {#each stagedFiles as file (fileKey(file))}
          <li
            class:selected={selectedFile?.staged === true && selectedFile.path === fileKey(file)}
            oncontextmenu={(event) => handleStagedContextMenu(event, file)}
          >
            <button type="button" class="file-row" onclick={() => selectFile(file, true)}>
              <FileStatusIcon status={file.status} />
              <span class="path">{fileLabel(file)}</span>
              <DiffStat insertions={file.insertions} deletions={file.deletions} />
            </button>
            <div class="file-actions">
              <button
                type="button"
                class="stage-toggle"
                title="Unstage"
                onclick={(event) => handleUnstageFile(file, event)}
              >
                −
              </button>
            </div>
          </li>
        {/each}
      </ul>
    </section>

    <ResizeHandle
      orientation="horizontal"
      onResize={(dy) => setStagedHeight(sectionHeightsState.staged + dy)}
    />
  </div>

  <form class="commit-box" onsubmit={handleCommit}>
    <div class="commit-title-row">
      <label for="commit-title">Commit title</label>
      <div class="title-row-actions">
        <button
          type="button"
          class="generate-ai"
          class:generating
          title={generateAiTooltip}
          aria-label={generateAiTooltip}
          disabled={!generating && aiDisabledReason !== null}
          onclick={generating ? handleStopGeneration : handleGenerateWithAi}
        >
          <Icon name={generating ? "square" : "sparkles"} size={12} />
          {generating ? "Stop" : "Generate"}
        </button>
        <span class="char-count" class:over={title.length >= TITLE_MAX_LENGTH}>
          {title.length}/{TITLE_MAX_LENGTH}
        </span>
      </div>
    </div>
    <input
      id="commit-title"
      type="text"
      bind:value={title}
      oninput={handleTitleInputWhileGenerating}
      onkeydown={handleFormKeydown}
      maxlength={TITLE_MAX_LENGTH}
      placeholder="Summarize this change"
      required
    />
    <label for="commit-description">Description (optional)</label>
    <textarea
      id="commit-description"
      bind:value={description}
      oninput={handleDescriptionInputWhileGenerating}
      onkeydown={handleFormKeydown}
      rows="3"
      placeholder="Add more detail"></textarea>
    <div class="commit-actions">
      <label class="amend">
        <input type="checkbox" checked={amend} onchange={handleAmendToggle} />
        Amend last commit
      </label>
      <label class="skip-hooks" class:active={skipHooks}>
        <input type="checkbox" bind:checked={skipHooks} onchange={handleSkipHooksChange} />
        Skip hooks
      </label>
      <div class="commit-submit">
        <Button
          variant="filled"
          type="submit"
          disabled={committing || commitDisabledReason !== null}
          title={commitDisabledReason ?? undefined}
        >
          {amend ? "Amend" : "Commit"}
        </Button>
      </div>
    </div>
    {#if aiError}
      <p class="error" role="alert">{aiError}</p>
    {/if}
    {#if commitError}
      <p class="error" role="alert">{commitError}</p>
    {/if}
  </form>
</div>

<style>
  .staging {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .scroll-sections {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }

  .file-group {
    display: flex;
    flex-direction: column;
    flex: 0 0 auto;
    overflow: hidden;
  }

  .file-group h3 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
    margin: 0 0 0.35rem;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-muted);
  }

  .stage-all {
    margin-left: auto;
    font: inherit;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: none;
    padding: 0.2rem 0.5rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .stage-all:hover:not(:disabled) {
    background: var(--surface-2);
    color: var(--accent);
  }

  .stage-all:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .file-group ul {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .file-group li {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
  }

  .file-group li:hover,
  .file-group li:focus-within {
    background: var(--surface-2);
  }

  .file-group li.selected,
  .file-group li.selected:hover,
  .file-group li.selected:focus-within {
    background: var(--accent-bg);
  }

  .file-row {
    flex: 1 1 auto;
    display: flex;
    gap: 0.5rem;
    align-items: center;
    min-width: 0;
    background: none;
    border: none;
    padding: 0.25rem 0.4rem;
    font: inherit;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
  }

  .file-actions {
    position: absolute;
    right: 0.25rem;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding-left: 1.5rem;
    background: linear-gradient(to right, transparent, var(--surface-2) 1.2rem);
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.1s ease;
  }

  .file-group li:hover .file-actions,
  .file-group li:focus-within .file-actions {
    opacity: 1;
    pointer-events: auto;
  }

  .file-group li.selected .file-actions {
    background: linear-gradient(to right, transparent, var(--accent-bg) 1.2rem);
  }

  .path {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stage-toggle {
    flex-shrink: 0;
    width: 1.6rem;
    height: 1.6rem;
    line-height: 1;
    margin-right: 8px;
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface-1);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .stage-toggle:hover {
    background: var(--surface-2);
    color: var(--accent);
  }

  .resolve-conflict {
    flex-shrink: 0;
    padding: 0.15rem 0.5rem;
    font-size: 0.72rem;
    line-height: 1.3;
    color: var(--danger);
    border-radius: var(--radius-sm);
    border: 1px solid var(--danger);
    background: var(--danger-bg);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .resolve-conflict:hover {
    background: var(--danger);
    color: var(--surface-0);
  }

  .error {
    margin: 0 0 0.5rem;
    color: var(--danger);
    font-size: 0.85rem;
  }

  .commit-box {
    display: flex;
    flex-direction: column;
    flex: 0 0 auto;
    gap: 0.4rem;
    border-top: 1px solid var(--border);
    padding-top: 0.6rem;
  }

  .commit-box label[for="commit-title"],
  .commit-box label[for="commit-description"] {
    font-size: 0.8rem;
    color: var(--text-muted);
  }

  .commit-title-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .title-row-actions {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .generate-ai {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    flex-shrink: 0;
    font: inherit;
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.2rem 0.5rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition:
      background-color 0.1s ease,
      color 0.1s ease;
  }

  .generate-ai:hover:not(:disabled),
  .generate-ai:focus-visible {
    background: var(--surface-2);
    color: var(--accent);
  }

  .generate-ai.generating {
    color: var(--danger);
    border-color: var(--danger);
  }

  .generate-ai:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .char-count {
    font-size: 0.72rem;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .char-count.over {
    color: var(--danger);
  }

  .commit-box input[type="text"],
  .commit-box textarea {
    font: inherit;
    font-size: 0.85rem;
    padding: 0.5rem;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .commit-box textarea {
    resize: vertical;
    min-height: 3.6rem;
  }

  .commit-box input[type="text"]:focus-visible,
  .commit-box textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .commit-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.75rem;
    row-gap: 0.4rem;
  }

  .amend,
  .skip-hooks {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .skip-hooks {
    color: var(--text-muted);
  }

  .skip-hooks.active {
    color: var(--danger);
  }

  .commit-submit {
    margin-left: auto;
  }
</style>
