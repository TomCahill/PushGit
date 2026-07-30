<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Notification history: every notifySuccess()/notifyError() call not yet marked viewed
  // (`toast.svelte.ts`'s `notificationHistory`), independent of the transient toast stack's own
  // auto-dismiss timer. "Mark as viewed" removes the entry outright rather than toggling a
  // read/unread flag — there's nothing to keep around once it's been seen. Hosted as a
  // `Popover`'s `children` from `RepoRail.svelte`'s Notifications button, so this component
  // itself doesn't know it's in a popover — same "dumb content, host owns the chrome" split as
  // `BranchSidebar`/`TagsPanel`/etc. inside `ActionRail`'s popovers.
  import { markAllNotificationsViewed, markNotificationViewed, notificationHistory } from "./toast.svelte";
  import Icon from "./Icon.svelte";

  function formatTime(firedAt: number): string {
    return new Date(firedAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
</script>

<div class="notifications-panel">
  <div class="notifications-header">
    <span class="notifications-title">Notifications</span>
    <button
      type="button"
      class="mark-all"
      onclick={markAllNotificationsViewed}
      disabled={notificationHistory.entries.length === 0}
    >
      Mark all as viewed
    </button>
  </div>

  {#if notificationHistory.entries.length === 0}
    <p class="placeholder">No notifications yet.</p>
  {:else}
    <ul class="notification-list">
      {#each notificationHistory.entries as entry (entry.id)}
        <li class="notification" class:success={entry.kind === "success"} class:error={entry.kind === "error"}>
          <div class="notification-body">
            <p class="notification-message">{entry.message}</p>
            <span class="notification-time">{formatTime(entry.firedAt)}</span>
          </div>
          <button
            type="button"
            class="mark-viewed"
            title="Mark as viewed"
            onclick={() => markNotificationViewed(entry.id)}
          >
            <Icon name="check" size={12} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .notifications-panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    min-width: 16rem;
  }

  .notifications-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .notifications-title {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .mark-all {
    font: inherit;
    font-size: 0.7rem;
    padding: 0.2rem 0.5rem;
    color: var(--text-secondary);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .mark-all:not(:disabled):hover {
    background: var(--accent-bg);
  }

  .mark-all:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .placeholder {
    margin: 0;
    color: var(--text-muted);
    font-size: 0.82rem;
  }

  .notification-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }

  .notification {
    display: flex;
    align-items: flex-start;
    gap: 0.4rem;
    padding: 0.4rem 0.5rem;
    background: var(--accent-bg);
    border-left: 3px solid var(--text-muted);
    border-radius: var(--radius-sm);
  }

  .notification.success {
    border-left-color: var(--success);
  }

  .notification.error {
    border-left-color: var(--danger);
  }

  .notification-body {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .notification-message {
    margin: 0;
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  .notification-time {
    font-size: 0.68rem;
    color: var(--text-muted);
  }

  .mark-viewed {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.3rem;
    height: 1.3rem;
    padding: 0;
    color: var(--text-secondary);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 0.1s ease;
  }

  .mark-viewed:hover {
    background: var(--surface-2);
    color: var(--accent);
  }
</style>
