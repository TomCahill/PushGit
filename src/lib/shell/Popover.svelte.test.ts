// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import PopoverHarness from "./PopoverHarness.svelte";

// The panel stays mounted at all times (see `Popover.svelte`'s header comment for why) and
// is only ever hidden via the `.hidden` class, not removed from the DOM — so these check
// visibility via that class rather than DOM presence.
function isPanelOpen(container: HTMLElement): boolean {
  const panel = container.querySelector(".panel");
  return panel !== null && !panel.classList.contains("hidden");
}

describe("Popover", () => {
  it("renders the trigger, with the panel hidden until opened", () => {
    const { getByText, container } = render(PopoverHarness);

    expect(getByText("Open me")).toBeTruthy();
    expect(isPanelOpen(container)).toBe(false);
  });

  it("shows the panel content after clicking the trigger, and hides it again on a second click", async () => {
    const { getByText, container } = render(PopoverHarness);

    await fireEvent.click(getByText("Open me"));
    expect(isPanelOpen(container)).toBe(true);
    expect(getByText("panel content")).toBeTruthy();

    await fireEvent.click(getByText("Open me"));
    expect(isPanelOpen(container)).toBe(false);
  });

  it("closes when clicking outside the popover", async () => {
    const { getByText, getByTestId, container } = render(PopoverHarness);

    await fireEvent.click(getByText("Open me"));
    expect(isPanelOpen(container)).toBe(true);

    await fireEvent.pointerDown(getByTestId("outside"));
    expect(isPanelOpen(container)).toBe(false);
  });

  it("closes on Escape", async () => {
    const { getByText, container } = render(PopoverHarness);

    await fireEvent.click(getByText("Open me"));
    expect(isPanelOpen(container)).toBe(true);

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(isPanelOpen(container)).toBe(false);
  });

  it("does not close when clicking inside the panel", async () => {
    const { getByText, container } = render(PopoverHarness);

    await fireEvent.click(getByText("Open me"));
    await fireEvent.pointerDown(getByText("panel content"));

    expect(isPanelOpen(container)).toBe(true);
  });

  it("keeps the panel content mounted while closed, not just hidden from the accessibility tree", () => {
    // The whole point of always-mounted content: BranchSidebar/TagsPanel/StashPanel keep
    // fetching and reporting summary data (current branch, counts) even before the user
    // ever opens the dropdown.
    const { getByText } = render(PopoverHarness);

    expect(getByText("panel content")).toBeTruthy();
  });

  describe("portal mode", () => {
    // RepoRail's Notifications button renders with `portal` so its panel escapes the left
    // rail's `overflow: auto` clipping — moved to `document.body` instead of staying nested
    // under the trigger, so these checks look at `document.body`/`window`, not `container`.
    it("moves the panel out to document.body instead of nesting it under the trigger", () => {
      const { container } = render(PopoverHarness, { props: { portal: true } });

      expect(container.querySelector(".panel")).toBeNull();
      expect(document.body.querySelector(".panel")).toBeTruthy();
    });

    it("still opens, and does not close when clicking inside the portaled panel", async () => {
      const { getByText } = render(PopoverHarness, { props: { portal: true } });

      await fireEvent.click(getByText("Open me"));
      const panel = document.body.querySelector(".panel");
      expect(panel?.classList.contains("hidden")).toBe(false);

      await fireEvent.pointerDown(getByText("panel content"));
      expect(panel?.classList.contains("hidden")).toBe(false);
    });

    it("closes on outside click and on Escape same as the non-portal case", async () => {
      const { getByText, getByTestId } = render(PopoverHarness, { props: { portal: true } });

      await fireEvent.click(getByText("Open me"));
      const panel = document.body.querySelector(".panel");
      expect(panel?.classList.contains("hidden")).toBe(false);

      await fireEvent.pointerDown(getByTestId("outside"));
      expect(panel?.classList.contains("hidden")).toBe(true);

      await fireEvent.click(getByText("Open me"));
      expect(panel?.classList.contains("hidden")).toBe(false);
      await fireEvent.keyDown(window, { key: "Escape" });
      expect(panel?.classList.contains("hidden")).toBe(true);
    });

    it("removes the portaled panel from document.body when the component unmounts", () => {
      const { unmount } = render(PopoverHarness, { props: { portal: true } });

      expect(document.body.querySelector(".panel")).toBeTruthy();
      unmount();
      expect(document.body.querySelector(".panel")).toBeNull();
    });
  });
});
