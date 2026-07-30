// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import Page from "./+page.svelte";
import { makeCommitRow, makeFileDiff } from "$lib/git/testFixtures";

describe("app shell", () => {
  it("renders the sidebar, commit graph, and diff panes", () => {
    mockIPC(() => null);
    const { getByLabelText } = render(Page);

    expect(getByLabelText("Branches, tags, and stashes")).toBeTruthy();
    expect(getByLabelText("Commit graph")).toBeTruthy();
    expect(getByLabelText("Diff viewer")).toBeTruthy();
  });

  it("opens a repository, lists its commits, and shows the selected commit's files", async () => {
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return "/repo";
        case "open_repository":
          return "/repo";
        case "start_repo_watcher":
          return null;
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [
              makeCommitRow({
                oid: "a",
                shortOid: "aaaaaaa",
                summary: "Initial commit",
                body: "Longer explanation of why this commit exists.",
              }),
            ],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "diff_commit":
          expect(args).toMatchObject({ repoPath: "/repo", commitOid: "a" });
          return [
            { oldPath: null, newPath: "README.md", status: "added", isBinary: false, hunks: [] },
          ];
        case "diff_unstaged":
        case "diff_staged":
        case "list_branches":
        case "list_conflicts":
          return [];
        case "repository_state":
          return "clean";
        case "plugin:event|listen":
          return 1;
        case "plugin:event|unlisten":
        case "stop_repo_watcher":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { getByTitle, findByText } = render(Page);
    await fireEvent.click(getByTitle("Open a repository"));

    const commitRow = await findByText("Initial commit");
    await fireEvent.click(commitRow);

    expect(await findByText("README.md", { exact: false })).toBeTruthy();
    expect(await findByText("Longer explanation of why this commit exists.")).toBeTruthy();
  });

  it("clicking a file in the sidebar shows its diff in the center panel, replacing the graph", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return "/repo";
        case "open_repository":
          return "/repo";
        case "start_repo_watcher":
          return null;
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", shortOid: "aaaaaaa", summary: "Initial commit" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "diff_commit":
          return [makeFileDiff({ newPath: "README.md" })];
        case "diff_unstaged":
        case "diff_staged":
        case "list_branches":
        case "list_conflicts":
          return [];
        case "repository_state":
          return "clean";
        case "plugin:event|listen":
          return 1;
        case "plugin:event|unlisten":
        case "stop_repo_watcher":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const {
      getByTitle,
      getByLabelText,
      queryByLabelText,
      findByText,
      findByRole,
      findByLabelText,
    } = render(Page);
    await fireEvent.click(getByTitle("Open a repository"));

    await fireEvent.click(await findByText("Initial commit"));
    const fileRow = await findByText("README.md");
    expect(getByLabelText("Commit graph")).toBeTruthy();

    await fireEvent.click(fileRow);

    expect(await findByText("hello", { exact: false })).toBeTruthy();
    expect(queryByLabelText("Commit graph")).toBeNull();

    await fireEvent.click(await findByRole("button", { name: /back to graph/i }));

    expect(await findByLabelText("Commit graph")).toBeTruthy();
  });

  it("loads a HEAD comparison when 'Diff against HEAD' is chosen from the graph's context menu", async () => {
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return "/repo";
        case "open_repository":
          return "/repo";
        case "start_repo_watcher":
          return null;
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", shortOid: "aaaaaaa", summary: "Initial commit" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "diff_commit":
          return [makeFileDiff({ newPath: "README.md" })];
        case "diff_between_commits":
          expect(args).toMatchObject({ repoPath: "/repo", fromOid: "a", toOid: "HEAD" });
          return [makeFileDiff({ newPath: "other.md" })];
        case "diff_unstaged":
        case "diff_staged":
        case "list_branches":
        case "list_conflicts":
          return [];
        case "repository_state":
          return "clean";
        case "plugin:event|listen":
          return 1;
        case "plugin:event|unlisten":
        case "stop_repo_watcher":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { getByTitle, findByText, findByRole } = render(Page);
    await fireEvent.click(getByTitle("Open a repository"));

    const commitRow = await findByText("Initial commit");
    await fireEvent.contextMenu(commitRow);
    await fireEvent.click(await findByRole("menuitem", { name: "Diff against HEAD" }));

    expect(await findByText("other.md", { exact: false })).toBeTruthy();
    expect(await findByText("— vs HEAD", { exact: false })).toBeTruthy();
  });

  it("shows blame in the center panel, with history diffs also swapping in there", async () => {
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "plugin:dialog|open":
          return "/repo";
        case "open_repository":
          return "/repo";
        case "start_repo_watcher":
          return null;
        case "graph_open":
          return "session-1";
        case "graph_page":
          return { rows: [], hasMore: false };
        case "graph_close":
          return null;
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "README.md" })];
        case "diff_staged":
        case "list_branches":
        case "list_conflicts":
          return [];
        case "repository_state":
          return "clean";
        case "blame_file":
          expect(args).toMatchObject({ repoPath: "/repo", path: "README.md" });
          return [
            {
              lineNo: 1,
              content: "hello world",
              oid: "aaa",
              shortOid: "aaa",
              authorName: "Ada",
              authorTime: 1700000000,
              summary: "first",
            },
          ];
        case "file_history":
          return [
            {
              oid: "aaa",
              shortOid: "aaa",
              summary: "first commit",
              authorName: "Ada",
              authorTime: 1700000000,
            },
          ];
        case "diff_commit":
          return [makeFileDiff({ newPath: "README.md" })];
        case "plugin:event|listen":
          return 1;
        case "plugin:event|unlisten":
        case "stop_repo_watcher":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const {
      getByTitle,
      getByLabelText,
      queryByLabelText,
      findByText,
      findByRole,
      findByLabelText,
      findByTitle,
    } = render(Page);
    await fireEvent.click(getByTitle("Open a repository"));

    await fireEvent.click(await findByTitle("Blame"));

    // The graph is replaced by the annotated file, not tucked into the (now history-only) sidebar.
    expect(queryByLabelText("Commit graph")).toBeNull();
    expect(await findByText("hello world")).toBeTruthy();
    expect(await findByText("first commit")).toBeTruthy();

    await fireEvent.click(await findByText("first commit"));

    // Selecting a history entry swaps the center panel to that commit's diff...
    expect(await findByText("hello", { exact: false })).toBeTruthy();
    const backButton = await findByRole("button", { name: /back to blame/i });

    await fireEvent.click(backButton);

    // ...and going back returns to the annotated file, still in the center panel.
    expect(await findByText("hello world")).toBeTruthy();
    expect(queryByLabelText("Commit graph")).toBeNull();

    await fireEvent.click(await findByRole("button", { name: "Close" }));

    expect(await findByLabelText("Commit graph")).toBeTruthy();
  });

  it("surfaces an error when the given path isn't a repository", async () => {
    mockIPC((cmd) => {
      if (cmd === "plugin:dialog|open") return "/not-a-repo";
      if (cmd === "open_repository") throw "git error: not a git repository";
      if (cmd === "stop_repo_watcher") return null;
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByTitle, findByRole } = render(Page);
    await fireEvent.click(getByTitle("Open a repository"));

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a git repository");
  });
});
