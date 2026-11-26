<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  import { tick } from "svelte";
  import { listBranches, listRemoteBranches, listTags, resolveRev } from "$lib/git/api";
  import type { TreeSource } from "$lib/git/types";
  import Icon, { type IconName } from "$lib/shell/Icon.svelte";
  import { WORKING_TREE, shortOid } from "./explore.svelte";

  let {
    repoPath,
    source,
    refLabel,
    open = $bindable(false),
    onPick,
  }: {
    repoPath: string;
    source: TreeSource;
    refLabel: string | null;
    open?: boolean;
    onPick: (source: TreeSource, refLabel: string | null) => void;
  } = $props();

  type Option = {
    section: string;
    label: string;
    icon: IconName;
    rev: string | null;
    head?: boolean;
  };

  let query = $state("");
  let options = $state<Option[]>([]);
  let activeIndex = $state(0);
  let error = $state<string | null>(null);
  let inputEl: HTMLInputElement | undefined = $state();
  let rootEl: HTMLDivElement | undefined = $state();
  const listId = `ref-list-${Math.random().toString(36).slice(2)}`;

  $effect(() => {
    if (!open) return;
    query = "";
    error = null;
    activeIndex = 0;
    void loadOptions();
    void tick().then(() => inputEl?.focus());
  });

  async function loadOptions() {
    const [branches, remotes, tags] = await Promise.all([
      listBranches(repoPath).catch(() => []),
      listRemoteBranches(repoPath).catch(() => []),
      listTags(repoPath).catch(() => []),
    ]);
    options = [
      { section: "", label: "Working tree", icon: "monitor", rev: null },
      ...branches.map((b) => ({
        section: "Local branches",
        label: b.name,
        icon: "branch" as const,
        rev: b.name,
        head: b.isHead,
      })),
      ...remotes.map((name) => ({
        section: "Remote branches",
        label: name,
        icon: "branch" as const,
        rev: name,
      })),
      ...tags.map((name) => ({ section: "Tags", label: name, icon: "tag" as const, rev: name })),
    ];
  }

  const filtered = $derived(
    query.trim()
      ? options.filter((o) => o.label.toLowerCase().includes(query.trim().toLowerCase()))
      : options,
  );

  $effect(() => {
    void filtered;
    activeIndex = 0;
  });

  async function pick(rev: string | null) {
    if (rev === null) {
      onPick(WORKING_TREE, null);
      open = false;
      return;
    }
    try {
      const oid = await resolveRev(repoPath, rev);
      const isSha = oid.startsWith(rev.toLowerCase());
      onPick({ kind: "revision", oid }, isSha ? null : rev);
      open = false;
    } catch (err) {
      error = String(err);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      activeIndex = Math.min(filtered.length - 1, activeIndex + 1);
    } else if (event.key === "ArrowUp") {
      activeIndex = Math.max(0, activeIndex - 1);
    } else if (event.key === "Enter") {
      const typed = query.trim();
      const exact = filtered.find((o) => o.label === typed);
      if (exact) void pick(exact.rev);
      else if (filtered.length > 0 && !looksLikeRev(typed)) void pick(filtered[activeIndex].rev);
      else if (typed) void pick(typed);
    } else if (event.key === "Escape") {
      open = false;
    } else {
      return;
    }
    event.preventDefault();
  }

  function looksLikeRev(text: string): boolean {
    return /^[0-9a-f]{4,40}$/i.test(text) || /[~^@]/.test(text);
  }

  function handleFocusOut(event: FocusEvent) {
    if (!rootEl?.contains(event.relatedTarget as Node | null)) open = false;
  }

  const buttonLabel = $derived(
    source.kind === "workingTree" ? "Working tree" : (refLabel ?? shortOid(source.oid)),
  );
</script>

<div class="ref-combobox" bind:this={rootEl} onfocusout={handleFocusOut}>
  <button
    type="button"
    class="ref-button"
    class:open
    aria-haspopup="listbox"
    aria-expanded={open}
    title={source.kind === "revision" ? source.oid : undefined}
    onclick={() => (open = !open)}
  >
    <Icon
      name={source.kind === "workingTree" ? "monitor" : refLabel ? "branch" : "clock"}
      size={14}
    />
    <span>{buttonLabel}</span>
    {#if source.kind === "revision" && refLabel}
      <span class="sha">{shortOid(source.oid)}</span>
    {/if}
    <Icon name="chevron-down" size={12} />
  </button>

  {#if open}
    <div class="popup">
      <label class="field" class:invalid={error}>
        <Icon name="search" size={14} />
        <input
          bind:this={inputEl}
          bind:value={query}
          oninput={() => (error = null)}
          onkeydown={handleKeydown}
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-activedescendant={filtered[activeIndex] ? `${listId}-${activeIndex}` : undefined}
          aria-label="Browse files at"
          aria-invalid={error ? "true" : undefined}
          placeholder="Branch, tag, SHA or HEAD~3"
        />
      </label>
      {#if error}
        <p class="error" role="alert"><Icon name="alert-triangle" size={14} /> {error}</p>
      {/if}
      <ul id={listId} role="listbox" aria-label="Refs">
        {#each filtered as option, i (option.section + option.label)}
          {#if i === 0 || filtered[i - 1].section !== option.section}
            {#if option.section}
              <li class="section" role="presentation">{option.section}</li>
            {/if}
          {/if}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id="{listId}-{i}"
            class="option"
            class:active={i === activeIndex}
            role="option"
            aria-selected={i === activeIndex}
            onmousedown={(event) => event.preventDefault()}
            onmouseenter={() => (activeIndex = i)}
            onclick={() => void pick(option.rev)}
          >
            <Icon name={option.icon} size={14} />
            <span class="label">{option.label}</span>
            {#if option.head}<span class="head">HEAD</span>{/if}
          </li>
        {/each}
      </ul>
      <p class="hint">Or type a SHA or HEAD~3 and press Enter</p>
    </div>
  {/if}
</div>

<style>
  .ref-combobox {
    position: relative;
  }

  .ref-button {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.75rem 0.4rem 0.6rem;
    font: inherit;
    font-size: 0.85rem;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .ref-button.open {
    border-color: var(--accent);
  }

  .sha {
    font-family: var(--font-mono);
    font-size: 0.78rem;
    font-weight: 400;
    color: var(--text-secondary);
  }

  .popup {
    position: absolute;
    z-index: 30;
    top: calc(100% + 0.4rem);
    left: 0;
    width: 24rem;
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0.5rem;
    padding: 0.45rem 0.75rem;
    background: var(--surface-0);
    border: 1px solid var(--accent);
    border-radius: 12px;
    color: var(--text-secondary);
  }

  .field.invalid {
    border-color: var(--danger);
  }

  input {
    flex: 1 1 auto;
    min-width: 0;
    font: inherit;
    font-size: 0.85rem;
    color: var(--text-primary);
    background: none;
    border: none;
    outline: none;
  }

  .error {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0 0.75rem 0.5rem;
    font-size: 0.8rem;
    color: var(--danger);
  }

  ul {
    margin: 0;
    padding: 0 0.3rem 0.3rem;
    list-style: none;
    overflow-y: auto;
  }

  .section {
    padding: 0.5rem 0.6rem 0.2rem;
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .option {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.4rem 0.6rem;
    font-size: 0.85rem;
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    cursor: pointer;
  }

  .option.active {
    background: var(--accent-bg);
  }

  .label {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .head {
    padding: 0 0.4rem;
    font-size: 0.7rem;
    border-radius: 0.6rem;
    background: var(--accent-bg);
  }

  .hint {
    margin: 0;
    padding: 0.4rem 0.9rem 0.6rem;
    border-top: 1px solid var(--border);
    font-size: 0.78rem;
    color: var(--text-secondary);
  }
</style>
