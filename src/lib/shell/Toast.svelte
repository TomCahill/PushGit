<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders whatever `toast.svelte.ts`'s `toastState.toasts` currently holds — mounted once
  // at the shell root so any panel can call `notifySuccess()`/`notifyError()` instead of
  // relying on its own inline feedback, which can go invisible when a hosting `Popover`
  // closes. See that file's header comment for the full reasoning.
  //
  // Anchored bottom-left and rising upward so it reads as emerging from `RepoRail.svelte`'s
  // Notifications button, which sits at the bottom of the left rail with left-aligned content —
  // a fixed offset lines up with it regardless of the (resizable) rail's width, no
  // `getBoundingClientRect`/`ResizeObserver` needed (same pure-CSS-anchoring philosophy as
  // `Popover.svelte`).
  import { fade, fly } from "svelte/transition";
  import { flip } from "svelte/animate";
  import { dismissToast, toastState } from "./toast.svelte";
  import { motionFast } from "./motion";
</script>

{#if toastState.toasts.length > 0}
  <div class="toast-stack" role="status" aria-live="polite">
    {#each toastState.toasts as toast (toast.id)}
      <div
        class="toast"
        class:success={toast.kind === "success"}
        class:error={toast.kind === "error"}
        in:fly={{ duration: motionFast(), y: 20 }}
        out:fade={{ duration: motionFast() }}
        animate:flip={{ duration: motionFast() }}
      >
        <p class="message">{toast.message}</p>
        <button
          type="button"
          class="dismiss"
          aria-label="Dismiss notification"
          onclick={() => dismissToast(toast.id)}
        >
          ×
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-stack {
    position: fixed;
    left: 1rem;
    bottom: 1rem;
    z-index: 200;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    max-width: min(22rem, calc(100vw - 2rem));
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.55rem 0.7rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-left: 3px solid var(--text-muted);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md);
  }

  .toast.success {
    border-left-color: var(--success);
  }

  .toast.error {
    border-left-color: var(--danger);
  }

  .message {
    flex: 1 1 auto;
    margin: 0;
    font-size: 0.82rem;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  .dismiss {
    flex-shrink: 0;
    padding: 0;
    font: inherit;
    font-size: 1rem;
    line-height: 1;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
  }

  .dismiss:hover {
    color: var(--text-primary);
  }
</style>
