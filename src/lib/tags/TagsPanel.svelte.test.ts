// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import TagsPanel from "./TagsPanel.svelte";

describe("TagsPanel", () => {
  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { getByText } = render(TagsPanel, { props: { repoPath: "", refreshKey: 0 } });

    expect(getByText("No tags.")).toBeTruthy();
  });

  it("loads and renders the tag list", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "list_tags":
          return ["v1.0.0", "v1.1.0"];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(TagsPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    expect(await findByText("v1.0.0")).toBeTruthy();
    expect(await findByText("v1.1.0")).toBeTruthy();
  });

  it("creates a tag at HEAD via the form and notifies the parent", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_tags":
          return [];
        case "create_tag":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByLabelText, getByRole } = render(TagsPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    const input = (await findByLabelText("New tag")) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: "v2.0.0" } });
    await fireEvent.click(getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", name: "v2.0.0", at: null, message: null }]),
    );
    await waitFor(() => expect(input.value).toBe(""));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("creates an annotated tag at a specific commit when target/message are filled in", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_tags":
          return [];
        case "create_tag":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(TagsPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.input(await findByLabelText("New tag"), { target: { value: "v2.0.0" } });
    await fireEvent.input(await findByLabelText("Target commit"), {
      target: { value: "abc123" },
    });
    await fireEvent.input(await findByLabelText("Tag message"), {
      target: { value: "Release 2.0.0" },
    });
    await fireEvent.click(getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(calls).toEqual([
        { repoPath: "/repo", name: "v2.0.0", at: "abc123", message: "Release 2.0.0" },
      ]),
    );
  });

  it("deletes a tag only after the user confirms", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "list_tags":
          return ["v1.0.0"];
        case "delete_tag":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByTitle, findByRole } = render(TagsPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle("Delete v1.0.0"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(calls).toEqual([]);

    await fireEvent.click(await findByTitle("Delete v1.0.0"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo", name: "v1.0.0" }]));
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_tags") throw "git error: not a repository";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(TagsPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
