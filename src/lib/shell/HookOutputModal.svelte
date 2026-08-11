<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders whatever `hookOutput.svelte.ts`'s `hookOutputState.session` currently holds —
  // mounted once at the shell root (`+page.svelte`), same pattern as `ConfirmDialog`/`Toast`.
  // Closing (×/Escape/backdrop click) just hides the transcript; it never cancels the
  // underlying commit/push (push keeps its own cancel button in `RemotePanel`, commit hooks
  // aren't cancellable). Auto-scrolls to the newest line unless the user has scrolled up to
  // read earlier output, matching ordinary terminal-log UX.
  import { tick } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { closeHookOutput, hookOutputState } from "./hookOutput.svelte";
  import { motionBase } from "./motion";

  let logEl: HTMLDivElement | undefined = $state();
  let pinnedToBottom = $state(true);

  function handleScroll() {
    if (!logEl) return;
    const distanceFromBottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight;
    pinnedToBottom = distanceFromBottom < 24;
  }

  $effect(() => {
    const session = hookOutputState.session;
    if (!session) return;
    // Re-run whenever a line is appended.
    void session.lines.length;
    if (!pinnedToBottom) return;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  $effect(() => {
    if (hookOutputState.session) pinnedToBottom = true;
  });

  function handleWindowKeydown(event: KeyboardEvent) {
    if (!hookOutputState.session?.visible) return;
    if (event.key === "Escape") {
      event.preventDefault();
      closeHookOutput();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) closeHookOutput();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if hookOutputState.session?.visible}
  {@const session = hookOutputState.session}
  <div
    class="backdrop"
    onclick={handleBackdropClick}
    role="presentation"
    transition:fade={{ duration: motionBase() }}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-label="{session.label} output"
      transition:scale={{ duration: motionBase(), start: 0.96 }}
    >
      <div class="header">
        <h2 class="title">{session.label}</h2>
        <span class="status" class:running={session.running}>
          {session.running ? "Running…" : "Done"}
        </span>
        <button type="button" class="close" aria-label="Close" onclick={closeHookOutput}>
          ×
        </button>
      </div>
      <div class="log" bind:this={logEl} onscroll={handleScroll} role="log" aria-live="polite">
        {#each session.lines as line, i (i)}
          <div class="line" class:stderr={line.stream === "stderr"}>{line.text}</div>
        {:else}
          <p class="empty">Waiting for output…</p>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.35);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(40rem, calc(100vw - 2rem));
    height: min(24rem, calc(100vh - 4rem));
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }

  .title {
    margin: 0;
    font-size: 0.9rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .status {
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .status.running {
    color: var(--accent);
  }

  .close {
    margin-left: auto;
    flex-shrink: 0;
    padding: 0 0.3rem;
    font: inherit;
    font-size: 1.1rem;
    line-height: 1;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
  }

  .close:hover {
    color: var(--text-primary);
  }

  .log {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 0.6rem 0.75rem;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .line {
    color: var(--text-primary);
  }

  .line.stderr {
    color: var(--danger);
  }

  .empty {
    margin: 0;
    color: var(--text-muted);
  }
</style>
