// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import CommitGraph from "./CommitGraph.svelte";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import ContextMenu from "$lib/shell/ContextMenu.svelte";
import { firePointer } from "$lib/shell/testPointerEvents";
import { makeCommitRow } from "$lib/git/testFixtures";

describe("CommitGraph", () => {
  it("shows a prompt instead of opening a session when no repo is given", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { getByText } = render(CommitGraph, { props: { repoPath: "" } });

    expect(getByText("Open a repository to see its commit graph.")).toBeTruthy();
  });

  it("opens a graph session and renders the returned rows", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [
              makeCommitRow({ oid: "a", shortOid: "aaaaaaa", summary: "First commit" }),
              makeCommitRow({ oid: "b", shortOid: "bbbbbbb", summary: "Second commit", row: 1 }),
            ],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });

    expect(await findByText("First commit")).toBeTruthy();
    expect(await findByText("Second commit")).toBeTruthy();
  });

  it("selects a commit on click and reports it via onSelect", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", summary: "Clickable commit" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onSelect = vi.fn();
    const { findByText } = render(CommitGraph, { props: { repoPath: "/repo", onSelect } });

    const row = await findByText("Clickable commit");
    await fireEvent.click(row);

    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ oid: "a" }));
  });

  it("copies the full SHA via its row's copy button without selecting the row", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [
              makeCommitRow({ oid: "abcdef1234567890", shortOid: "abcdef1", summary: "A commit" }),
            ],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    const onSelect = vi.fn();
    const { findByText, findByRole } = render(CommitGraph, {
      props: { repoPath: "/repo", onSelect },
    });

    await findByText("A commit");
    await fireEvent.click(await findByRole("button", { name: "Copy commit SHA" }));

    expect(writeText).toHaveBeenCalledWith("abcdef1234567890");
    // onSelect(null) already fires once on mount, clearing any prior selection — what
    // matters here is that the copy click never selects *this* commit.
    expect(onSelect).not.toHaveBeenCalledWith(expect.objectContaining({ oid: "abcdef1234567890" }));
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "graph_open") throw "git error: not a repository";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(CommitGraph, { props: { repoPath: "/not-a-repo" } });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });

  it("renders a tag ref exactly like a branch ref, inline at its target commit", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [
              makeCommitRow({
                oid: "a",
                summary: "Tagged commit",
                refs: [
                  { name: "main", kind: "local_branch", isHead: true },
                  { name: "v1.0.0", kind: "tag", isHead: false },
                ],
              }),
            ],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });

    expect(await findByText("main")).toBeTruthy();
    expect(await findByText("v1.0.0")).toBeTruthy();
  });

  it("requests the next page once the scroll position nears the loaded bottom", async () => {
    let pageCalls = 0;
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          pageCalls += 1;
          if (pageCalls === 1) {
            return { rows: [makeCommitRow({ oid: "a", summary: "Page one" })], hasMore: true };
          }
          return {
            rows: [makeCommitRow({ oid: "b", summary: "Page two", row: 1 })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { container, findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
    await findByText("Page one");

    const scroller = container.querySelector(".graph-scroll") as HTMLElement;
    Object.defineProperty(scroller, "scrollHeight", { value: 24, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 24, configurable: true });
    Object.defineProperty(scroller, "scrollTop", { value: 0, configurable: true });

    await fireEvent.scroll(scroller);

    expect(await findByText("Page two")).toBeTruthy();
    expect(pageCalls).toBe(2);
  });

  it("keeps previously-rendered rows visible while refetching after a same-repo refreshKey bump", async () => {
    let pageCall = 0;
    const deferredSecondPage: { resolve: ((value: unknown) => void) | null } = { resolve: null };
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          pageCall += 1;
          if (pageCall === 1) {
            return {
              rows: [makeCommitRow({ oid: "a", summary: "Original commit" })],
              hasMore: false,
            };
          }
          return new Promise((resolve) => {
            deferredSecondPage.resolve = resolve;
          });
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, queryByText, rerender } = render(CommitGraph, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });
    await findByText("Original commit");

    await rerender({ repoPath: "/repo", refreshKey: 1 });

    // The refetch triggered by the refreshKey bump is in flight — its graph_page promise
    // hasn't resolved yet — but the old row must still be on screen. This used to blank
    // immediately on every refreshKey bump (barely visible on a tiny repo, a glaring
    // "flash" on a large one) because the parent shell force-remounted the whole component.
    expect(queryByText("Original commit")).toBeTruthy();
    // Nor should a "Loading…" line flash below it — on a repo with frequent watcher-driven
    // refreshes (e.g. active local edits) this used to flicker on every single bump even
    // though the rows above it never actually went away.
    expect(queryByText("Loading…")).toBeNull();

    deferredSecondPage.resolve?.({
      rows: [makeCommitRow({ oid: "b", summary: "Refreshed commit" })],
      hasMore: false,
    });

    expect(await findByText("Refreshed commit")).toBeTruthy();
    expect(queryByText("Original commit")).toBeNull();
  });

  it("clears rows immediately when repoPath actually changes, not just refreshKey", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", summary: "Original commit" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, queryByText, rerender } = render(CommitGraph, {
      props: { repoPath: "/repo-one", refreshKey: 0 },
    });
    await findByText("Original commit");

    await rerender({ repoPath: "/repo-two", refreshKey: 0 });

    expect(queryByText("Original commit")).toBeNull();
  });

  describe("right-click context menus", () => {
    it("opens the commit-dot menu and checks out the commit", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return { rows: [makeCommitRow({ oid: "a", summary: "A commit" })], hasMore: false };
          case "graph_close":
            return null;
          case "checkout_commit":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      const row = await findByText("A commit");
      await fireEvent.contextMenu(row);

      for (const label of [
        "Checkout this commit",
        "Create branch here",
        "Create tag here",
        "Cherry-pick this commit",
        "Copy full SHA",
        "Copy summary",
        "Diff against working tree",
        "Diff against HEAD",
      ]) {
        expect(await findByRole("menuitem", { name: label })).toBeTruthy();
      }

      await fireEvent.click(await findByRole("menuitem", { name: "Checkout this commit" }));

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", oid: "a" }]));
    });

    it("cherry-picks a commit from the commit-dot menu and reports the outcome", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return { rows: [makeCommitRow({ oid: "a", summary: "A commit" })], hasMore: false };
          case "graph_close":
            return null;
          case "cherry_pick_commit":
            calls.push(args);
            return { kind: "cherry_picked", oid: "new-oid" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      const row = await findByText("A commit");
      await fireEvent.contextMenu(row);
      await fireEvent.click(await findByRole("menuitem", { name: "Cherry-pick this commit" }));

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", commitOid: "a" }]));
      expect(await findByText("Cherry-picked onto the current branch.")).toBeTruthy();
    });

    it("reports conflicts from a cherry-pick and notifies the parent", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return { rows: [makeCommitRow({ oid: "a", summary: "A commit" })], hasMore: false };
          case "graph_close":
            return null;
          case "cherry_pick_commit":
            return { kind: "conflicts", conflicts: ["a.txt"] };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const onConflicts = vi.fn();
      const { findByText, findByRole } = render(CommitGraph, {
        props: { repoPath: "/repo", onConflicts },
      });

      const row = await findByText("A commit");
      await fireEvent.contextMenu(row);
      await fireEvent.click(await findByRole("menuitem", { name: "Cherry-pick this commit" }));

      expect(await findByText(/Cherry-pick stopped with 1 conflicting/)).toBeTruthy();
      await waitFor(() => expect(onConflicts).toHaveBeenCalled());
    });

    it("copies the commit summary from the commit-dot menu without a git call", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [makeCommitRow({ oid: "a", summary: "Copy me", refs: [] })],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("Copy me"));
      await fireEvent.click(await findByRole("menuitem", { name: "Copy summary" }));

      expect(writeText).toHaveBeenCalledWith("Copy me");
    });

    it("fires onDiffRequest, not onSelect, for the diff-mode menu actions", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return { rows: [makeCommitRow({ oid: "a", summary: "A commit" })], hasMore: false };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onDiffRequest = vi.fn();
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, {
        props: { repoPath: "/repo", onDiffRequest },
      });

      await fireEvent.contextMenu(await findByText("A commit"));
      await fireEvent.click(await findByRole("menuitem", { name: "Diff against HEAD" }));

      expect(onDiffRequest).toHaveBeenCalledWith(expect.objectContaining({ oid: "a" }), "head");
    });

    it("shows only Push for a local branch badge that is the current HEAD", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "main", kind: "local_branch", isHead: true }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const { findByText, findByRole, queryByRole } = render(CommitGraph, {
        props: { repoPath: "/repo" },
      });

      await fireEvent.contextMenu(await findByText("main"));

      expect(await findByRole("menuitem", { name: "Push to origin" })).toBeTruthy();
      expect(queryByRole("menuitem", { name: "Checkout branch" })).toBeNull();
      expect(queryByRole("menuitem", { name: "Merge into current branch" })).toBeNull();
      expect(queryByRole("menuitem", { name: "Delete branch" })).toBeNull();
    });

    it("deletes a non-HEAD local branch from its badge's menu after confirming", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "feature", kind: "local_branch", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "delete_branch":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ConfirmDialog);
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("feature"));
      await fireEvent.click(await findByRole("menuitem", { name: "Delete branch" }));
      await fireEvent.click(
        within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
      );

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "feature" }]));
    });

    it("exercises checkout/fetch/pull/merge from a remote branch badge's menu", async () => {
      const calls: Record<string, unknown[]> = {
        checkout_remote_branch: [],
        fetch: [],
        pull: [],
        merge_branch: [],
      };
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "origin/feature", kind: "remote_branch", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "checkout_remote_branch":
            calls.checkout_remote_branch.push(args);
            return null;
          case "fetch": {
            const { repoPath, remoteName } = args as { repoPath: string; remoteName: string };
            calls.fetch.push({ repoPath, remoteName });
            return null;
          }
          case "pull": {
            const { repoPath, remoteName } = args as { repoPath: string; remoteName: string };
            calls.pull.push({ repoPath, remoteName });
            return { kind: "already_up_to_date" };
          }
          case "merge_branch":
            calls.merge_branch.push(args);
            return { kind: "already_up_to_date" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const badge = await findByText("origin/feature");

      await fireEvent.contextMenu(badge);
      await fireEvent.click(
        await findByRole("menuitem", { name: "Checkout (creates local branch)" }),
      );

      await fireEvent.contextMenu(badge);
      await fireEvent.click(await findByRole("menuitem", { name: "Fetch origin" }));

      await fireEvent.contextMenu(badge);
      await fireEvent.click(await findByRole("menuitem", { name: "Pull current branch" }));

      await fireEvent.contextMenu(badge);
      await fireEvent.click(await findByRole("menuitem", { name: "Merge into current branch" }));

      await waitFor(() => {
        expect(calls.checkout_remote_branch).toEqual([
          { repoPath: "/repo", remoteBranchName: "origin/feature" },
        ]);
        expect(calls.fetch).toEqual([{ repoPath: "/repo", remoteName: "origin" }]);
        expect(calls.pull).toEqual([{ repoPath: "/repo", remoteName: "origin" }]);
        expect(calls.merge_branch).toEqual([{ repoPath: "/repo", branchName: "origin/feature" }]);
      });
    });

    it("selects the commit when opening a tag badge's menu", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "v1.0.0", kind: "tag", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onSelect = vi.fn();
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, {
        props: { repoPath: "/repo", onSelect },
      });

      await fireEvent.contextMenu(await findByText("v1.0.0"));

      expect(await findByRole("menuitem", { name: "Rename tag" })).toBeTruthy();
      expect(await findByRole("menuitem", { name: "Delete tag" })).toBeTruthy();
      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ oid: "a" }));
    });

    it("renames a tag from its badge's menu", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "v1.0.0", kind: "tag", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "rename_tag":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ConfirmDialog);
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("v1.0.0"));
      await fireEvent.click(await findByRole("menuitem", { name: "Rename tag" }));

      const dialog = await findByRole("alertdialog");
      const input = within(dialog).getByRole("textbox");
      await fireEvent.input(input, { target: { value: "v1.0.1" } });
      await fireEvent.click(within(dialog).getByRole("button", { name: "OK" }));

      await waitFor(() =>
        expect(calls).toEqual([{ repoPath: "/repo", oldName: "v1.0.0", newName: "v1.0.1" }]),
      );
    });

    it("deletes a tag from its badge's menu after confirming", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "v1.0.0", kind: "tag", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "delete_tag":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ConfirmDialog);
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("v1.0.0"));
      await fireEvent.click(await findByRole("menuitem", { name: "Delete tag" }));
      await fireEvent.click(
        within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
      );

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "v1.0.0" }]));
    });

    it("calls onConflicts when a menu-triggered merge hits conflicts", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "feature", kind: "local_branch", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "merge_branch":
            return { kind: "conflicts", conflicts: ["a.txt"] };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onConflicts = vi.fn();
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, {
        props: { repoPath: "/repo", onConflicts },
      });

      await fireEvent.contextMenu(await findByText("feature"));
      await fireEvent.click(await findByRole("menuitem", { name: "Merge into current branch" }));

      await waitFor(() => expect(onConflicts).toHaveBeenCalled());
    });
  });

  describe("workdir and stash rows", () => {
    it("routes a click on the workdir row to onSelectWorkdir instead of onSelect", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "0".repeat(40),
                  kind: "workdir",
                  summary: "Uncommitted changes (2 files)",
                  parents: ["a"],
                }),
                makeCommitRow({ oid: "a", summary: "Head commit", row: 1 }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onSelect = vi.fn();
      const onSelectWorkdir = vi.fn();
      const { findByText } = render(CommitGraph, {
        props: { repoPath: "/repo", onSelect, onSelectWorkdir },
      });

      onSelect.mockClear();
      await fireEvent.click(await findByText("Uncommitted changes (2 files)"));

      expect(onSelectWorkdir).toHaveBeenCalledOnce();
      expect(onSelect).not.toHaveBeenCalled();
    });

    it("gets no context menu at all on the workdir row", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "0".repeat(40),
                  kind: "workdir",
                  summary: "Uncommitted changes (1 file)",
                  parents: ["a"],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ContextMenu);
      const { findByText, queryByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("Uncommitted changes (1 file)"));

      expect(queryByRole("menuitem")).toBeNull();
    });

    it("shows a stash@{N} badge and diffs it like a normal commit on click", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "s",
                  kind: "stash",
                  stashIndex: 0,
                  summary: "WIP on develop: a last commit",
                  parents: ["a"],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onSelect = vi.fn();
      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo", onSelect } });

      expect(await findByText("stash@{0}")).toBeTruthy();
      await fireEvent.click(await findByText("WIP on develop: a last commit"));

      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ oid: "s", kind: "stash" }));
    });

    it("applies a stash from its row's context menu", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "s",
                  kind: "stash",
                  stashIndex: 2,
                  summary: "WIP on develop",
                  parents: ["a"],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "apply_stash":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onApplied = vi.fn();
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, {
        props: { repoPath: "/repo", onApplied },
      });

      await fireEvent.contextMenu(await findByText("WIP on develop"));
      await fireEvent.click(await findByRole("menuitem", { name: "Apply stash" }));

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", index: 2 }]));
      expect(onApplied).toHaveBeenCalledOnce();
    });

    it("drops a stash from its row's context menu after confirming", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "s",
                  kind: "stash",
                  stashIndex: 0,
                  summary: "WIP on develop",
                  parents: ["a"],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "drop_stash":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      render(ConfirmDialog);
      render(ContextMenu);
      const { findByText, findByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });

      await fireEvent.contextMenu(await findByText("WIP on develop"));
      await fireEvent.click(await findByRole("menuitem", { name: "Drop stash" }));
      await fireEvent.click(
        within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
      );

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", index: 0 }]));
    });
  });

  describe("drag and drop merge/rebase", () => {
    it("merges a non-current branch dropped onto the current branch's badge", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "feature", kind: "local_branch", isHead: false },
                    { name: "main", kind: "local_branch", isHead: true },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "merge_branch":
            calls.push(args);
            return { kind: "already_up_to_date" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("feature");
      const targetBadge = await findByText("main");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", branchName: "feature" }]));
    });

    it("ignores a right-button drag over a ref badge, leaving the button free for its context menu", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "feature", kind: "local_branch", isHead: false },
                    { name: "main", kind: "local_branch", isHead: true },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "merge_branch":
            calls.push(args);
            return { kind: "already_up_to_date" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("feature");
      const targetBadge = await findByText("main");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1, button: 2 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1, button: 2 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1, button: 2 });

      expect(calls).toEqual([]);
    });

    it("rebases the current branch dropped onto a non-current branch's badge", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "main", kind: "local_branch", isHead: true },
                    { name: "feature", kind: "local_branch", isHead: false },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "rebase_branch":
            calls.push(args);
            return { kind: "completed" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("main");
      const targetBadge = await findByText("feature");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", onto: "feature" }]));
    });

    it("rebases the current branch onto a bare commit dropped from its badge", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "Current commit",
                  refs: [{ name: "main", kind: "local_branch", isHead: true }],
                }),
                makeCommitRow({ oid: "b", summary: "Target commit", row: 1 }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "rebase_branch":
            calls.push(args);
            return { kind: "completed" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("main");
      const targetSummary = await findByText("Target commit");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetSummary);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 0, clientY: 30, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 0, clientY: 30, pointerId: 1 });

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", onto: "b" }]));
    });

    it("does not merge or rebase when dragging between two non-current branches", async () => {
      const mergeCalls: unknown[] = [];
      const rebaseCalls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "feature-a", kind: "local_branch", isHead: false },
                    { name: "feature-b", kind: "local_branch", isHead: false },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "merge_branch":
            mergeCalls.push(args);
            return { kind: "already_up_to_date" };
          case "rebase_branch":
            rebaseCalls.push(args);
            return { kind: "completed" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("feature-a");
      const targetBadge = await findByText("feature-b");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      expect(mergeCalls).toEqual([]);
      expect(rebaseCalls).toEqual([]);
    });

    it("does not start a drag for an ordinary click with no movement past the threshold", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [{ name: "feature", kind: "local_branch", isHead: false }],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onSelect = vi.fn();
      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo", onSelect } });
      const badge = await findByText("feature");

      firePointer(badge, "pointerdown", { clientX: 10, clientY: 10, pointerId: 1 });
      firePointer(badge, "pointermove", { clientX: 11, clientY: 10, pointerId: 1 });
      firePointer(badge, "pointerup", { clientX: 11, clientY: 10, pointerId: 1 });
      await fireEvent.click(badge);

      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ oid: "a" }));
    });

    it("suppresses the trailing click synthesized after a completed drag", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "feature", kind: "local_branch", isHead: false },
                    { name: "main", kind: "local_branch", isHead: true },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "merge_branch":
            return { kind: "already_up_to_date" };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onSelect = vi.fn();
      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo", onSelect } });
      const sourceBadge = await findByText("feature");
      const targetBadge = await findByText("main");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });
      await fireEvent.click(targetBadge);

      // onSelect(null) already fires once on mount (see the copy-button test above) — what
      // matters is that the trailing click never selects *this* commit a second time.
      expect(onSelect).not.toHaveBeenCalledWith(expect.objectContaining({ oid: "a" }));
    });

    it("calls onConflicts for a drag-triggered rebase that hits conflicts", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "main", kind: "local_branch", isHead: true },
                    { name: "feature", kind: "local_branch", isHead: false },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "rebase_branch":
            return { kind: "conflicts", conflicts: ["a.txt"] };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const onConflicts = vi.fn();
      const { findByText } = render(CommitGraph, {
        props: { repoPath: "/repo", onConflicts },
      });
      const sourceBadge = await findByText("main");
      const targetBadge = await findByText("feature");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      await waitFor(() => expect(onConflicts).toHaveBeenCalled());
    });

    it("moves a tag dropped onto a bare commit row", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "Tagged commit",
                  refs: [{ name: "v1.0.0", kind: "tag", isHead: false }],
                }),
                makeCommitRow({ oid: "b", summary: "Target commit", row: 1 }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "move_tag":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("v1.0.0");
      const targetRow = await findByText("Target commit");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetRow);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 24, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 24, pointerId: 1 });

      await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "v1.0.0", to: "b" }]));
    });

    it("moves a tag dropped onto a branch badge", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "v1.0.0", kind: "tag", isHead: false },
                    { name: "main", kind: "local_branch", isHead: true },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "move_tag":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("v1.0.0");
      const targetBadge = await findByText("main");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      await waitFor(() =>
        expect(calls).toEqual([{ repoPath: "/repo", name: "v1.0.0", to: "main" }]),
      );
    });

    it("does not move a tag dropped onto another tag", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "graph_open":
            return "session-1";
          case "graph_page":
            return {
              rows: [
                makeCommitRow({
                  oid: "a",
                  summary: "A commit",
                  refs: [
                    { name: "v1.0.0", kind: "tag", isHead: false },
                    { name: "v2.0.0", kind: "tag", isHead: false },
                  ],
                }),
              ],
              hasMore: false,
            };
          case "graph_close":
            return null;
          case "move_tag":
            throw new Error("move_tag should not be called");
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText, queryByRole } = render(CommitGraph, { props: { repoPath: "/repo" } });
      const sourceBadge = await findByText("v1.0.0");
      const targetBadge = await findByText("v2.0.0");
      vi.spyOn(document, "elementFromPoint").mockReturnValue(targetBadge);

      firePointer(sourceBadge, "pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointermove", { clientX: 20, clientY: 0, pointerId: 1 });
      firePointer(sourceBadge, "pointerup", { clientX: 20, clientY: 0, pointerId: 1 });

      // `decideDragAction` returns null for tag-onto-tag, so `handlePointerUp` never calls
      // `move_tag` at all — if it somehow had, the mocked throw above would have surfaced as
      // an error banner here.
      expect(queryByRole("alert")).toBeNull();
    });
  });
});
