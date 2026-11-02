// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import CommandPalette from "./CommandPalette.svelte";
import { closeCommandPalette, paletteState } from "./commandPalette.svelte";
import { makeBranchInfo, makeCommitRow, makeFileDiff } from "$lib/git/testFixtures";

afterEach(() => {
  closeCommandPalette();
});

describe("CommandPalette", () => {
  it("opens on Ctrl+P and loads branches/files", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, findByText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [] },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });

    expect(await findByLabelText("Command palette search")).toBeTruthy();
    expect(await findByText("main (current)")).toBeTruthy();
    expect(await findByText("feature")).toBeTruthy();
    expect(await findByText("a.txt")).toBeTruthy();
  });

  it("closes on Escape", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [];
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, queryByLabelText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [] },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    const input = await findByLabelText("Command palette search");
    await fireEvent.keyDown(input, { key: "Escape" });

    expect(queryByLabelText("Command palette search")).toBeNull();
  });

  it("filters entries by the typed query across sections", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "release" }),
          ];
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, findByText, queryByText } = render(CommandPalette, {
      props: {
        repoPath: "/repo",
        commands: [{ id: "fetch", label: "Fetch from origin", run: () => {} }],
      },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    const input = (await findByLabelText("Command palette search")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "rel" } });

    expect(await findByText("release")).toBeTruthy();
    expect(queryByText("main (current)")).toBeNull();
    expect(queryByText("Fetch from origin")).toBeNull();
  });

  it("checks out a branch on click, notifies the parent, and closes", async () => {
    const checkoutCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "feature" }),
          ];
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        case "checkout_branch":
          checkoutCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByLabelText, findByText, queryByLabelText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [], onChanged },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    await findByLabelText("Command palette search");
    await fireEvent.click(await findByText("feature"));

    await waitFor(() => expect(checkoutCalls).toEqual([{ repoPath: "/repo", name: "feature" }]));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    await waitFor(() => expect(queryByLabelText("Command palette search")).toBeNull());
  });

  it("runs a supplied command and closes the palette", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [];
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const run = vi.fn();
    const { findByLabelText, findByText, queryByLabelText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [{ id: "fetch", label: "Fetch from origin", run }] },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    await findByLabelText("Command palette search");
    await fireEvent.click(await findByText("Fetch from origin"));

    await waitFor(() => expect(run).toHaveBeenCalled());
    await waitFor(() => expect(queryByLabelText("Command palette search")).toBeNull());
  });

  it("selecting a file switches to the working-directory view and closes", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_branches":
          return [];
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onSelectFile = vi.fn();
    const { findByLabelText, findByText, queryByLabelText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [], onSelectFile },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    await findByLabelText("Command palette search");
    await fireEvent.click(await findByText("a.txt"));

    expect(onSelectFile).toHaveBeenCalled();
    await waitFor(() => expect(queryByLabelText("Command palette search")).toBeNull());
  });

  it("searches commits as the query is typed, and copies the SHA on select", async () => {
    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    let openCalls = 0;
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_branches":
          return [];
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        case "graph_open":
          openCalls += 1;
          expect((args as { filter: { search: string } }).filter.search).toBe("fix bug");
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "deadbeef", shortOid: "deadbe", summary: "fix bug" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, findByText, queryByLabelText } = render(CommandPalette, {
      props: { repoPath: "/repo", commands: [] },
    });

    await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
    const input = (await findByLabelText("Command palette search")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "fix bug" } });

    await waitFor(() => expect(openCalls).toBe(1), { timeout: 1000 });
    await fireEvent.click(await findByText("deadbe fix bug"));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("deadbeef"));
    await waitFor(() => expect(queryByLabelText("Command palette search")).toBeNull());
  });

  describe("repositories", () => {
    const repos = [
      { path: "/p/Lighten/project-q", lastOpenedAt: 1 },
      { path: "/p/Buttress/api", lastOpenedAt: 3 },
      { path: "/p/Lighten/nginx-config", lastOpenedAt: 2 },
    ];

    function mockEmptyRepo() {
      mockIPC((cmd) => {
        if (cmd === "list_branches" || cmd.startsWith("diff_")) return [];
        throw new Error(`unexpected command ${cmd}`);
      });
    }

    function optionLabels(container: HTMLElement): string[] {
      return [...container.querySelectorAll("[role=option]")].map((el) => el.textContent!.trim());
    }

    it("lists recent repos newest first, matching on the parent directory too", async () => {
      mockEmptyRepo();
      const { container, findByLabelText } = render(CommandPalette, {
        props: { repoPath: "/p/Lighten/project-q", commands: [], repos },
      });

      await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
      await findByLabelText("Command palette search");
      expect(optionLabels(container)).toEqual([
        "api · Buttress",
        "nginx-config · Lighten",
        "project-q · Lighten (current)",
      ]);

      await fireEvent.input(await findByLabelText("Command palette search"), {
        target: { value: "lighten" },
      });
      expect(optionLabels(container)).toEqual([
        "nginx-config · Lighten",
        "project-q · Lighten (current)",
      ]);
    });

    it("opens the chosen repo and closes, but doesn't reopen the current one", async () => {
      mockEmptyRepo();
      const onOpenRepo = vi.fn();
      const { findByLabelText, getByText } = render(CommandPalette, {
        props: { repoPath: "/p/Lighten/project-q", commands: [], repos, onOpenRepo },
      });

      await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
      const dialog = (await findByLabelText("Command palette search")).closest("[role=dialog]")!;
      await fireEvent.keyDown(dialog, { key: "Enter" });
      expect(onOpenRepo).toHaveBeenCalledWith("/p/Buttress/api");
      expect(paletteState.open).toBe(false);

      await fireEvent.keyDown(window, { key: "p", ctrlKey: true });
      await fireEvent.click(getByText("project-q · Lighten (current)"));
      expect(onOpenRepo).toHaveBeenCalledTimes(1);
      expect(paletteState.open).toBe(false);
    });
  });
});
