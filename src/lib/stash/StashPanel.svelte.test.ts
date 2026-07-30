// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import StashPanel from "./StashPanel.svelte";
import { makeStashEntry } from "$lib/git/testFixtures";

describe("StashPanel", () => {
  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { getByText } = render(StashPanel, { props: { repoPath: "", refreshKey: 0 } });

    expect(getByText("No stashes.")).toBeTruthy();
  });

  it("loads and renders the stash list", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_stashes":
          return [
            makeStashEntry({ index: 0, message: "On main: WIP", oid: "a".repeat(40) }),
            makeStashEntry({ index: 1, message: "On main: wip: refactor", oid: "b".repeat(40) }),
          ];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(StashPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    expect(await findByText("On main: WIP")).toBeTruthy();
    expect(await findByText("On main: wip: refactor")).toBeTruthy();
    expect(await findByText("aaaaaaa")).toBeTruthy();
  });

  it("creates a stash via the form and notifies the parent", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_stashes":
          return [];
        case "create_stash":
          calls.push(args);
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByLabelText, getByRole } = render(StashPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    const input = (await findByLabelText("Stash message (optional)")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "wip: testing" } });
    await fireEvent.click(getByRole("button", { name: "Stash changes" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", message: "wip: testing" }]));
    await waitFor(() => expect(input.value).toBe(""));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("applies a stash and notifies onApplied", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_stashes":
          return [makeStashEntry({ index: 0, message: "On main: WIP" })];
        case "apply_stash":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onApplied = vi.fn();
    const { findByTitle } = render(StashPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onApplied },
    });

    await fireEvent.click(await findByTitle('Apply "On main: WIP"'));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", index: 0 }]));
    await waitFor(() => expect(onApplied).toHaveBeenCalled());
  });

  it("pops a stash", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_stashes":
          return [makeStashEntry({ index: 0, message: "On main: WIP" })];
        case "pop_stash":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByTitle } = render(StashPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    await fireEvent.click(await findByTitle('Pop "On main: WIP"'));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", index: 0 }]));
  });

  it("drops a stash only after the user confirms", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_stashes":
          return [makeStashEntry({ index: 0, message: "On main: WIP" })];
        case "drop_stash":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(StashPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle('Drop "On main: WIP"'));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(calls).toEqual([]);

    await fireEvent.click(await findByTitle('Drop "On main: WIP"'));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", index: 0 }]));
  });

  it("renames a stash via a prompt", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_stashes":
          return [makeStashEntry({ index: 0, message: "On main: WIP" })];
        case "rename_stash":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(StashPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle('Rename "On main: WIP"'));
    const dialog = within(await findByRole("alertdialog"));
    const input = dialog.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("On main: WIP");
    await fireEvent.input(input, { target: { value: "renamed" } });
    await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", index: 0, newMessage: "renamed" }]),
    );
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_stashes") throw "git error: not a repository";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(StashPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
