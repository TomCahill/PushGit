// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import CherryPickRangePicker from "./CherryPickRangePicker.svelte";
import { makeCommitRow } from "$lib/git/testFixtures";

describe("CherryPickRangePicker", () => {
  it("searches, selects commits, and cherry-picks them in chronological order", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [
              makeCommitRow({ oid: "newer", shortOid: "newer", summary: "Newer", authorTime: 200 }),
              makeCommitRow({ oid: "older", shortOid: "older", summary: "Older", authorTime: 100 }),
            ],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "cherry_pick_range":
          calls.push(args);
          return { kind: "cherry_picked", oid: "new-oid" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onCompleted = vi.fn();
    const { findByLabelText, findByText, findByRole } = render(CherryPickRangePicker, {
      props: {
        repoPath: "/repo",
        onCompleted,
        onConflicts: vi.fn(),
        onCancel: vi.fn(),
      },
    });

    const search = await findByLabelText("Search commits to cherry-pick");
    await fireEvent.input(search, { target: { value: "commit" } });

    await findByText("Newer");
    // Select in reverse-chronological click order — the picker should still submit oldest first.
    await fireEvent.click((await findByText("Newer")).closest("li")!.querySelector("input")!);
    await fireEvent.click((await findByText("Older")).closest("li")!.querySelector("input")!);

    await fireEvent.click(await findByRole("button", { name: "Cherry-pick 2 commits" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", commitOids: ["older", "newer"] }]),
    );
    await waitFor(() => expect(onCompleted).toHaveBeenCalled());
  });

  it("shows a conflict banner and can continue or abort", async () => {
    const continueCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", shortOid: "a", summary: "A" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "cherry_pick_range":
          return { kind: "conflicts", conflicts: ["a.txt"] };
        case "continue_cherry_pick_range":
          continueCalls.push(args);
          return { kind: "cherry_picked", oid: "new-oid" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onConflicts = vi.fn();
    const onCompleted = vi.fn();
    const { findByLabelText, findByText, findByRole } = render(CherryPickRangePicker, {
      props: { repoPath: "/repo", onCompleted, onConflicts, onCancel: vi.fn() },
    });

    const search = await findByLabelText("Search commits to cherry-pick");
    await fireEvent.input(search, { target: { value: "a" } });
    await fireEvent.click((await findByText("A")).closest("li")!.querySelector("input")!);
    await fireEvent.click(await findByRole("button", { name: "Cherry-pick 1 commit" }));

    expect(await findByText(/1 conflicting file/)).toBeTruthy();
    await waitFor(() => expect(onConflicts).toHaveBeenCalled());

    await fireEvent.click(await findByRole("button", { name: "Continue cherry-pick" }));
    await waitFor(() => expect(continueCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(onCompleted).toHaveBeenCalled());
  });

  it("aborts a paused range", async () => {
    const abortCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "graph_open":
          return "session-1";
        case "graph_page":
          return {
            rows: [makeCommitRow({ oid: "a", shortOid: "a", summary: "A" })],
            hasMore: false,
          };
        case "graph_close":
          return null;
        case "cherry_pick_range":
          return { kind: "conflicts", conflicts: ["a.txt"] };
        case "abort_cherry_pick_range":
          abortCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onCancel = vi.fn();
    const { findByLabelText, findByText, findByRole } = render(CherryPickRangePicker, {
      props: { repoPath: "/repo", onCompleted: vi.fn(), onConflicts: vi.fn(), onCancel },
    });

    const search = await findByLabelText("Search commits to cherry-pick");
    await fireEvent.input(search, { target: { value: "a" } });
    await fireEvent.click((await findByText("A")).closest("li")!.querySelector("input")!);
    await fireEvent.click(await findByRole("button", { name: "Cherry-pick 1 commit" }));

    await findByRole("button", { name: "Abort cherry-pick" });
    await fireEvent.click(await findByRole("button", { name: "Abort cherry-pick" }));

    await waitFor(() => expect(abortCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
  });

  it("disables Cherry-pick until at least one commit is selected", async () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { findByRole } = render(CherryPickRangePicker, {
      props: { repoPath: "/repo", onCompleted: vi.fn(), onConflicts: vi.fn(), onCancel: vi.fn() },
    });

    const button = (await findByRole("button", {
      name: "Cherry-pick 0 commits",
    })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });
});
