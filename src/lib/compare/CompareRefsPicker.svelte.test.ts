// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import CompareRefsPicker from "./CompareRefsPicker.svelte";
import { makeFileDiff } from "$lib/git/testFixtures";

function mockRefLists() {
  return (cmd: string) => {
    switch (cmd) {
      case "list_branches":
        return [];
      case "list_tags":
        return [];
      case "list_remote_branches":
        return [];
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  };
}

describe("CompareRefsPicker", () => {
  it("disables Compare until both refs are filled in", async () => {
    mockIPC(mockRefLists());

    const { findByRole, findByLabelText } = render(CompareRefsPicker, {
      props: { repoPath: "/repo", onResult: vi.fn() },
    });

    const compareButton = (await findByRole("button", { name: "Compare" })) as HTMLButtonElement;
    expect(compareButton.disabled).toBe(true);

    await fireEvent.input(await findByLabelText("Compare against"), {
      target: { value: "feature" },
    });
    expect(compareButton.disabled).toBe(false);
  });

  it("swaps the two ref values", async () => {
    mockIPC(mockRefLists());

    const { findByLabelText, findByTitle } = render(CompareRefsPicker, {
      props: { repoPath: "/repo", onResult: vi.fn() },
    });

    const from = (await findByLabelText("Compare from")) as HTMLInputElement;
    const to = (await findByLabelText("Compare against")) as HTMLInputElement;
    await fireEvent.input(to, { target: { value: "feature" } });

    await fireEvent.click(await findByTitle("Swap"));

    expect(from.value).toBe("feature");
    expect(to.value).toBe("HEAD");
  });

  it("compares two refs and reports the resulting files", async () => {
    const calls: unknown[] = [];
    const files = [makeFileDiff({ newPath: "a.txt" })];
    mockIPC((cmd, args) => {
      if (cmd === "diff_between_commits") {
        calls.push(args);
        return files;
      }
      return mockRefLists()(cmd);
    });

    const onResult = vi.fn();
    const { findByRole, findByLabelText } = render(CompareRefsPicker, {
      props: { repoPath: "/repo", onResult },
    });

    await fireEvent.input(await findByLabelText("Compare against"), {
      target: { value: "feature" },
    });
    await fireEvent.click(await findByRole("button", { name: "Compare" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", fromOid: "HEAD", toOid: "feature" }]),
    );
    await waitFor(() => expect(onResult).toHaveBeenCalledWith(files, null, "HEAD", "feature"));
  });

  it("reports an error when the compare fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "diff_between_commits") {
        throw new Error("revspec 'bogus' not found");
      }
      return mockRefLists()(cmd);
    });

    const onResult = vi.fn();
    const { findByRole, findByLabelText } = render(CompareRefsPicker, {
      props: { repoPath: "/repo", onResult },
    });

    await fireEvent.input(await findByLabelText("Compare against"), {
      target: { value: "bogus" },
    });
    await fireEvent.click(await findByRole("button", { name: "Compare" }));

    await waitFor(() =>
      expect(onResult).toHaveBeenCalledWith(null, expect.stringContaining("bogus"), "HEAD", "bogus"),
    );
  });
});
