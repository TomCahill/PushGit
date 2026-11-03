// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import WelcomeView from "./WelcomeView.svelte";
import type { RecentRepo } from "./recentRepos";

function props(recentRepos: RecentRepo[] = []) {
  return {
    recentRepos,
    folders: [{ id: "f1", name: "Lighten" }],
    openError: null,
    onOpen: vi.fn(),
    onSelectRecent: vi.fn(),
  };
}

describe("WelcomeView", () => {
  it("lists the five most recently opened repos, newest first, with their folder", () => {
    const now = Date.now();
    const repos = ["a", "b", "c", "d", "e", "f"].map((name, i) => ({
      path: `/p/${name}`,
      lastOpenedAt: now - i * 60_000,
      folderId: name === "a" ? "f1" : undefined,
    }));

    const { getByRole } = render(WelcomeView, { props: props(repos.reverse()) });

    const list = getByRole("list", { name: "Recent repositories" });
    const names = [...list.querySelectorAll(".name")].map((el) => el.textContent);
    expect(names).toEqual(["a", "b", "c", "d", "e"]);
    expect(list.querySelector("li")?.textContent).toContain("Lighten");
  });

  it("hides Recent when there are no repos", () => {
    const { queryByRole, queryByText } = render(WelcomeView, { props: props() });

    expect(queryByRole("list")).toBeNull();
    expect(queryByText("Recent")).toBeNull();
  });

  it("opens a recent repo or the folder picker", async () => {
    const p = props([{ path: "/p/project-q", lastOpenedAt: Date.now() }]);
    const { getByRole } = render(WelcomeView, { props: p });

    await fireEvent.click(getByRole("button", { name: /project-q/ }));
    await fireEvent.click(getByRole("button", { name: /Open repository/ }));

    expect(p.onSelectRecent).toHaveBeenCalledWith("/p/project-q");
    expect(p.onOpen).toHaveBeenCalled();
  });

  it("shows an open error next to the button that fixes it", () => {
    const { getByRole } = render(WelcomeView, {
      props: { ...props(), openError: "Not a git repository" },
    });

    expect(getByRole("alert").textContent).toContain("Not a git repository");
  });
});
