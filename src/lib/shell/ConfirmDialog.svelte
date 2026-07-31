<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Renders whatever `confirmDialog.svelte.ts`'s `dialogState.request` currently holds —
  // mounted once at the shell root so every panel can `await confirmAsync(...)`/
  // `await promptAsync(...)` instead of the browser's native confirm()/prompt(), which
  // don't respect the app's theme.
  import { fade, scale } from "svelte/transition";
  import { dialogState } from "./confirmDialog.svelte";
  import { motionBase } from "./motion";
  import Button from "./Button.svelte";

  let inputValue = $state("");
  let inputEl: HTMLInputElement | undefined = $state();
  let confirmButtonEl: HTMLButtonElement | null = $state(null);

  $effect(() => {
    const request = dialogState.request;
    if (!request) return;
    if (request.kind === "prompt") {
      inputValue = request.defaultValue;
      inputEl?.select();
    } else {
      confirmButtonEl?.focus();
    }
  });

  function respondConfirm(result: boolean) {
    const request = dialogState.request;
    if (!request || request.kind !== "confirm") return;
    dialogState.request = null;
    request.resolve(result);
  }

  function respondPrompt(result: string | null) {
    const request = dialogState.request;
    if (!request || request.kind !== "prompt") return;
    dialogState.request = null;
    request.resolve(result);
  }

  function cancelCurrent() {
    if (dialogState.request?.kind === "prompt") respondPrompt(null);
    else respondConfirm(false);
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    const request = dialogState.request;
    if (!request) return;
    if (event.key === "Escape") {
      event.preventDefault();
      cancelCurrent();
    } else if (event.key === "Enter" && request.kind === "confirm") {
      event.preventDefault();
      respondConfirm(true);
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) cancelCurrent();
  }

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    respondPrompt(inputValue);
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if dialogState.request}
  {@const request = dialogState.request}
  <div
    class="backdrop"
    onclick={handleBackdropClick}
    role="presentation"
    transition:fade={{ duration: motionBase() }}
  >
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-label={request.message}
      transition:scale={{ duration: motionBase(), start: 0.96 }}
    >
      <p class="message">{request.message}</p>
      {#if request.kind === "prompt"}
        <form onsubmit={handleSubmit}>
          <input type="text" bind:value={inputValue} bind:this={inputEl} />
          <div class="actions">
            <Button variant="outlined" onclick={() => respondPrompt(null)}>Cancel</Button>
            <Button variant="filled" type="submit">OK</Button>
          </div>
        </form>
      {:else}
        <div class="actions">
          <Button variant="outlined" onclick={() => respondConfirm(false)}>Cancel</Button>
          <Button variant="filled" bind:ref={confirmButtonEl} onclick={() => respondConfirm(true)}>
            OK
          </Button>
        </div>
      {/if}
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
    width: min(24rem, calc(100vw - 2rem));
    padding: 1rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
  }

  .message {
    margin: 0 0 0.75rem;
    font-size: 0.9rem;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  input[type="text"] {
    width: 100%;
    box-sizing: border-box;
    padding: 0.4rem 0.55rem;
    font: inherit;
    font-size: 0.85rem;
    color: var(--text-primary);
    background: var(--surface-0);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  input[type="text"]:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }
</style>
