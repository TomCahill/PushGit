// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Transient success/error notifications, mirroring `confirmDialog.svelte.ts`'s imperative
// singleton pattern: call notifySuccess()/notifyError() from anywhere, rendered by the one
// <Toast /> mounted at the shell root (`+page.svelte`). Exists because several `ActionRail`
// panels (`BranchSidebar`/`TagsPanel`/`StashPanel`/`MaintenancePanel`/`RemotePanel`) render
// inside a `Popover` that closes on outside-click or Escape regardless of whether an action
// just failed or succeeded, so any outcome shown only inside the panel would be invisible
// until it's reopened. A toast surfaces the outcome outside whatever panel triggered it —
// it's the sole feedback mechanism for action results; panels don't duplicate it with their
// own inline success/error text.
//
// Every call here also mirrors to a native OS notification when the window is unfocused
// — a single hook point rather than
// touching each of the five panels' call sites. Gated on focus, not fired unconditionally: an
// action that just completed while the window is focused (the user clicked the button) has no
// need for a native ping; only something that finished while the user was elsewhere does.

import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

export type ToastKind = "success" | "error";

export type Toast = {
  id: number;
  kind: ToastKind;
  message: string;
};

export type NotificationEntry = {
  id: number;
  kind: ToastKind;
  message: string;
  firedAt: number;
};

const SUCCESS_DURATION_MS = 4000;
const ERROR_DURATION_MS = 6000;
// Bounds the session's notification history so a long-running session doesn't grow this
// unboundedly — history isn't persisted across restarts, so there's no need for it to be large.
const MAX_HISTORY_ENTRIES = 200;

export const toastState: { toasts: Toast[] } = $state({ toasts: [] });

// Every notifySuccess()/notifyError() call not yet marked viewed, kept regardless of whether/when
// its toast auto-dismisses — the Notifications panel (`RepoRail.svelte`) reads this, not
// `toastState`. There's no separate read/unread flag: marking a notification (or all of them) as
// viewed removes it from this list outright, so its length doubles as the unread count.
export const notificationHistory: { entries: NotificationEntry[] } = $state({ entries: [] });

let nextId = 0;

function push(kind: ToastKind, message: string, durationMs: number): void {
  const id = ++nextId;
  toastState.toasts = [...toastState.toasts, { id, kind, message }];
  setTimeout(() => dismissToast(id), durationMs);

  notificationHistory.entries = [
    { id, kind, message, firedAt: Date.now() },
    ...notificationHistory.entries,
  ].slice(0, MAX_HISTORY_ENTRIES);
}

/** Permission is requested lazily on first attempted send, not eagerly at startup, so the
 *  app never prompts before the user has done anything that would produce a notification.
 *  Every failure mode (no plugin support, permission denied, request rejected) just means
 *  "don't notify" — this must never be able to throw into a caller that isn't expecting it,
 *  same philosophy as `config`'s degrade-on-failure storage. */
async function notifyNative(title: string, body: string): Promise<void> {
  try {
    if (await getCurrentWindow().isFocused()) return;
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch {
    // No notification support in this context (e.g. tests, unsupported platform) — ignore.
  }
}

export function notifySuccess(message: string): void {
  push("success", message, SUCCESS_DURATION_MS);
  void notifyNative("PushGit", message);
}

export function notifyError(message: string): void {
  push("error", message, ERROR_DURATION_MS);
  void notifyNative("PushGit — Error", message);
}

export function dismissToast(id: number): void {
  toastState.toasts = toastState.toasts.filter((toast) => toast.id !== id);
}

export function markNotificationViewed(id: number): void {
  notificationHistory.entries = notificationHistory.entries.filter((entry) => entry.id !== id);
}

export function markAllNotificationsViewed(): void {
  notificationHistory.entries = [];
}

export function unreadNotificationCount(): number {
  return notificationHistory.entries.length;
}
