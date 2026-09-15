<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Ref-vs-ref compare: two free-text ref inputs (branch, tag, remote branch, or any
  // commit-ish like a short SHA or `HEAD~3`), backed by a `<datalist>` of known local
  // branches/tags/remote branches for convenience — the diff itself already accepts
  // anything `git2::Repository::revparse_single` resolves, so typed input isn't restricted
  // to the suggested list. Purely a picker: the actual `FileDiff[]` result (or error) is
  // reported to the caller via `onResult`, which owns rendering it (mirrors `CommitDiffView`
  // already being caller-owned for the plain commit-diff view).
  import { diffBetweenCommits, listBranches, listRemoteBranches, listTags } from "$lib/git/api";
  import Button from "$lib/shell/Button.svelte";
  import type { FileDiff } from "$lib/git/types";

  let {
    repoPath,
    onResult,
  }: {
    repoPath: string;
    /** `fromRef`/`toRef` are the exact strings compared, reported alongside the result so the
     *  caller can reuse them (e.g. for "open in external diff tool") without duplicating this
     *  component's own input state. */
    onResult: (
      files: FileDiff[] | null,
      error: string | null,
      fromRef: string,
      toRef: string,
    ) => void;
  } = $props();

  let from = $state("HEAD");
  let to = $state("");
  let refOptions = $state<string[]>([]);
  let busy = $state(false);

  $effect(() => {
    const path = repoPath;
    let cancelled = false;
    void (async () => {
      try {
        const [branches, tags, remoteBranches] = await Promise.all([
          listBranches(path),
          listTags(path),
          listRemoteBranches(path),
        ]);
        if (cancelled) return;
        refOptions = [...branches.map((b) => b.name), ...tags, ...remoteBranches];
      } catch {
        // Autocomplete is a convenience only — leave the inputs usable for manual entry.
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  let compareGeneration = 0;

  async function handleCompare() {
    const fromRef = from.trim();
    const toRef = to.trim();
    if (!fromRef || !toRef || busy) return;
    const myGeneration = ++compareGeneration;
    busy = true;
    try {
      const files = await diffBetweenCommits(repoPath, fromRef, toRef);
      if (myGeneration !== compareGeneration) return;
      onResult(files, null, fromRef, toRef);
    } catch (err) {
      if (myGeneration !== compareGeneration) return;
      onResult(null, String(err), fromRef, toRef);
    } finally {
      if (myGeneration === compareGeneration) busy = false;
    }
  }

  function handleSwap() {
    [from, to] = [to, from];
  }
</script>

<div class="compare-picker">
  <div class="ref-row">
    <input
      type="text"
      bind:value={from}
      list="compare-ref-options"
      placeholder="e.g. main"
      aria-label="Compare from"
    />
    <button type="button" class="swap" title="Swap" onclick={handleSwap} disabled={busy}>
      ⇄
    </button>
    <input
      type="text"
      bind:value={to}
      list="compare-ref-options"
      placeholder="e.g. feature-branch"
      aria-label="Compare against"
    />
    <Button
      variant="filled"
      onclick={handleCompare}
      disabled={busy || !from.trim() || !to.trim()}
    >
      {busy ? "Comparing…" : "Compare"}
    </Button>
  </div>
  <datalist id="compare-ref-options">
    {#each refOptions as name (name)}
      <option value={name}></option>
    {/each}
  </datalist>
</div>

<style>
  .compare-picker {
    display: flex;
    flex-direction: column;
  }

  .ref-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .ref-row input {
    flex: 1 1 auto;
    min-width: 0;
    font: inherit;
    font-size: 0.85rem;
    padding: 0.35rem 0.55rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }

  .ref-row input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .swap {
    flex-shrink: 0;
    width: 1.8rem;
    height: 1.8rem;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .swap:not(:disabled):hover {
    background: var(--surface-2);
  }
</style>
