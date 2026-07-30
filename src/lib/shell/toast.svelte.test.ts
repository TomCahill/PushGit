// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import Toast from "./Toast.svelte";
import {
  dismissToast,
  markAllNotificationsViewed,
  markNotificationViewed,
  notificationHistory,
  notifyError,
  notifySuccess,
  toastState,
  unreadNotificationCount,
} from "./toast.svelte";

describe("toast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    toastState.toasts = [];
    notificationHistory.entries = [];
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders nothing when there are no toasts", () => {
    const { container } = render(Toast);
    expect(container.querySelector(".toast-stack")).toBeNull();
  });

  it("shows a success toast and auto-dismisses it after its duration", async () => {
    const { findByText, queryByText } = render(Toast);

    notifySuccess("Fetched from origin.");
    expect(await findByText("Fetched from origin.")).toBeTruthy();

    // `*Async` rather than plain `advanceTimersByTime` + a bare `Promise.resolve()` — the
    // dismissed toast's exit now runs a real `transition:`/`animate:` (reduce-motion pass,
    // `.private/feature/reduce-motion-setting/PLAN.md`'s Phase C), which resolves over one or
    // more real microtask ticks (`src/test-setup.ts`'s `Element.prototype.animate` stub), and
    // only the `*Async` variant reliably drains those between/after advancing the fake clock.
    await vi.advanceTimersByTimeAsync(3999);
    expect(queryByText("Fetched from origin.")).toBeTruthy();

    await vi.advanceTimersByTimeAsync(1);
    expect(queryByText("Fetched from origin.")).toBeNull();
  });

  it("shows an error toast for longer than a success toast", async () => {
    const { queryByText } = render(Toast);

    notifyError("Push rejected: origin has changes you don't have locally.");

    await vi.advanceTimersByTimeAsync(4001);
    expect(queryByText("Push rejected: origin has changes you don't have locally.")).toBeTruthy();

    await vi.advanceTimersByTimeAsync(2000);
    expect(queryByText("Push rejected: origin has changes you don't have locally.")).toBeNull();
  });

  it("stacks multiple toasts and dismisses them independently", async () => {
    const { findByText, queryByText } = render(Toast);

    notifySuccess("Merged feature into main.");
    notifyError("Delete branch failed.");

    expect(await findByText("Merged feature into main.")).toBeTruthy();
    expect(await findByText("Delete branch failed.")).toBeTruthy();

    await vi.advanceTimersByTimeAsync(4000);
    expect(queryByText("Merged feature into main.")).toBeNull();
    expect(queryByText("Delete branch failed.")).toBeTruthy();
  });

  it("can be dismissed manually via its close button", async () => {
    const { findByText, findByLabelText, queryByText } = render(Toast);

    notifySuccess("Stash applied.");
    expect(await findByText("Stash applied.")).toBeTruthy();

    await fireEvent.click(await findByLabelText("Dismiss notification"));
    expect(queryByText("Stash applied.")).toBeNull();
  });

  it("dismissToast removes only the matching entry from the shared state", () => {
    notifySuccess("First.");
    notifySuccess("Second.");
    const [first, second] = toastState.toasts;

    dismissToast(first.id);

    expect(toastState.toasts).toEqual([second]);
  });

  it("records every notification to history, newest first, surviving auto-dismiss", async () => {
    notifySuccess("First.");
    notifyError("Second.");

    expect(notificationHistory.entries.map((entry) => entry.message)).toEqual([
      "Second.",
      "First.",
    ]);

    await vi.advanceTimersByTimeAsync(6000);
    expect(toastState.toasts).toEqual([]);
    expect(notificationHistory.entries.map((entry) => entry.message)).toEqual([
      "Second.",
      "First.",
    ]);
  });

  it("caps notification history at 200 entries, dropping the oldest", () => {
    for (let i = 0; i < 205; i++) notifySuccess(`Message ${i}.`);

    expect(notificationHistory.entries).toHaveLength(200);
    expect(notificationHistory.entries[0].message).toBe("Message 204.");
    expect(notificationHistory.entries.at(-1)!.message).toBe("Message 5.");
  });

  it("marks a single notification as viewed by removing it, without affecting others", () => {
    notifySuccess("First.");
    notifySuccess("Second.");
    const [second, first] = notificationHistory.entries;

    markNotificationViewed(first.id);

    expect(notificationHistory.entries.map((entry) => entry.id)).toEqual([second.id]);
    expect(unreadNotificationCount()).toBe(1);
  });

  it("marks all notifications as viewed by clearing the history", () => {
    notifySuccess("First.");
    notifyError("Second.");
    expect(unreadNotificationCount()).toBe(2);

    markAllNotificationsViewed();

    expect(unreadNotificationCount()).toBe(0);
    expect(notificationHistory.entries).toEqual([]);
  });
});
