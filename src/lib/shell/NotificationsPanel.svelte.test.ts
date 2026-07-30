// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import NotificationsPanel from "./NotificationsPanel.svelte";
import { notificationHistory, notifyError, notifySuccess, toastState } from "./toast.svelte";

describe("NotificationsPanel", () => {
  beforeEach(() => {
    toastState.toasts = [];
    notificationHistory.entries = [];
  });

  it("shows a placeholder when there's no history", () => {
    const { getByText, queryByRole } = render(NotificationsPanel);

    expect(getByText("No notifications yet.")).toBeTruthy();
    expect(queryByRole("listitem")).toBeNull();
  });

  it("lists fired notifications newest first, each with its message and a time", () => {
    notifySuccess("Fetched from origin.");
    notifyError("Push rejected.");

    const { getAllByRole } = render(NotificationsPanel);

    const items = getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0].textContent).toContain("Push rejected.");
    expect(items[1].textContent).toContain("Fetched from origin.");
    // A locale time string was rendered (exact format is locale-dependent, so just check it's
    // non-empty distinct text alongside the message).
    expect(items[0].querySelector(".notification-time")?.textContent?.length).toBeGreaterThan(0);
  });

  it("disables 'Mark all as viewed' when there's nothing unread", () => {
    const { getByRole } = render(NotificationsPanel);

    const button = getByRole("button", { name: "Mark all as viewed" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  it("marks a single notification as viewed by removing just that one", async () => {
    notifySuccess("Fetched from origin.");
    notifyError("Push rejected.");
    const [, older] = notificationHistory.entries;

    const { getAllByTitle, queryByText, getByText } = render(NotificationsPanel);

    await fireEvent.click(getAllByTitle("Mark as viewed")[0]);

    expect(queryByText("Push rejected.")).toBeNull();
    expect(getByText("Fetched from origin.")).toBeTruthy();
    expect(notificationHistory.entries.map((e) => e.id)).toEqual([older.id]);
  });

  it("marks every notification as viewed via the top button, clearing the list", async () => {
    notifySuccess("First.");
    notifyError("Second.");

    const { getByRole, queryAllByTitle, getByText } = render(NotificationsPanel);

    await fireEvent.click(getByRole("button", { name: "Mark all as viewed" }));

    expect(queryAllByTitle("Mark as viewed")).toHaveLength(0);
    expect(notificationHistory.entries).toEqual([]);
    expect(getByText("No notifications yet.")).toBeTruthy();
    const button = getByRole("button", { name: "Mark all as viewed" }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
