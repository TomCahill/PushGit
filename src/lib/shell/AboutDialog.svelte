<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Help > About — an in-app modal rather than a
  // native dialog, matching `ConfirmDialog.svelte`'s backdrop/dialog pattern so it respects the
  // app's theme like every other overlay here. Name/version are read live via the Tauri app API
  // instead of hardcoded, so they can't drift from `tauri.conf.json`.
  import { getName, getVersion } from "@tauri-apps/api/app";
  import Logo from "./Logo.svelte";

  let { open, onClose }: { open: boolean; onClose: () => void } = $props();

  let appName = $state("");
  let appVersion = $state("");

  $effect(() => {
    if (!open) return;
    getName()
      .then((n) => (appName = n))
      .catch(() => (appName = "PushGit"));
    getVersion()
      .then((v) => (appVersion = v))
      .catch(() => (appVersion = ""));
  });

  function handleWindowKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) onClose();
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if open}
  <div class="backdrop" onclick={handleBackdropClick} role="presentation">
    <div class="dialog" role="alertdialog" aria-modal="true" aria-label="About {appName}">
      <div class="logo"><Logo size={40} /></div>
      <h2 class="title">{appName}</h2>
      <p class="version">Version {appVersion}</p>
      <p class="tagline">A fast, native git GUI with a proper visual commit graph.</p>
      <p class="license">License: AGPL-3.0-or-later</p>
      <p class="author">© 2026 Tom Cahill</p>
      <div class="actions">
        <button type="button" class="primary" onclick={onClose}>Close</button>
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
    width: min(22rem, calc(100vw - 2rem));
    padding: 1.25rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-md);
    text-align: center;
  }

  .logo {
    display: flex;
    justify-content: center;
    margin-bottom: 0.6rem;
    color: var(--accent);
  }

  .title {
    margin: 0 0 0.35rem;
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .version {
    margin: 0 0 0.75rem;
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  .tagline {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
    color: var(--text-primary);
  }

  .license {
    margin: 0 0 0.35rem;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .author {
    margin: 0;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .actions {
    display: flex;
    justify-content: center;
    margin-top: 1rem;
  }

  .actions button {
    padding: 0.4rem 0.9rem;
    font: inherit;
    font-size: 0.85rem;
    font-weight: 500;
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .primary {
    color: var(--btn-filled-fg);
    background: var(--btn-filled-bg);
    border: none;
  }
</style>
