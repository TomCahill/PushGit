// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import SettingsPanel from "./SettingsPanel.svelte";
import {
  DEFAULT_MAX_COMMITS_RENDERED,
  MIN_MAX_COMMITS_RENDERED,
  settingsState,
} from "./settings.svelte";
import { toastState } from "$lib/shell/toast.svelte";

describe("SettingsPanel", () => {
  afterEach(() => {
    settingsState.maxCommitsRendered = DEFAULT_MAX_COMMITS_RENDERED;
    settingsState.reduceMotion = false;
    toastState.toasts = [];
  });

  it("shows the current max-commits-rendered value", () => {
    settingsState.maxCommitsRendered = 750;
    mockIPC(() => {
      throw new Error("should not be called when no repo is open");
    });

    const { getByLabelText } = render(SettingsPanel);

    expect((getByLabelText("Max commits rendered in graph") as HTMLInputElement).value).toBe("750");
  });

  it("does not show a 'This Repository' section when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called when no repo is open");
    });

    const { queryByText } = render(SettingsPanel);

    expect(queryByText("This Repository")).toBeNull();
  });

  it("saves a new max-commits value and shows a confirmation", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_max_commits_rendered") {
        expect(args).toEqual({ value: 1000 });
        return { maxCommitsRendered: 1000 };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText, getByText } = render(SettingsPanel);
    const input = getByLabelText("Max commits rendered in graph") as HTMLInputElement;

    await fireEvent.input(input, { target: { value: "1000" } });
    await fireEvent.click(getByText("Save"));

    expect(await waitFor(() => getByText("Saved."))).toBeTruthy();
  });

  it("clamps a value below the minimum on save", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_max_commits_rendered") {
        return { maxCommitsRendered: MIN_MAX_COMMITS_RENDERED };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText, getByText } = render(SettingsPanel);
    const input = getByLabelText("Max commits rendered in graph") as HTMLInputElement;

    await fireEvent.input(input, { target: { value: "5" } });
    await fireEvent.click(getByText("Save"));

    await waitFor(() => expect(settingsState.maxCommitsRendered).toBe(MIN_MAX_COMMITS_RENDERED));
    expect(input.value).toBe(String(MIN_MAX_COMMITS_RENDERED));
  });

  it("saves the reduce-motion preference as soon as the checkbox is toggled", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_reduce_motion") {
        expect(args).toEqual({ value: true });
        return { maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED, reduceMotion: true };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Reduce motion") as HTMLInputElement;

    await fireEvent.click(checkbox);

    expect(checkbox.checked).toBe(true);
  });

  it("reverts the reduce-motion checkbox and shows an error toast when the save fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_reduce_motion") throw "disk full";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Reduce motion") as HTMLInputElement;

    await fireEvent.click(checkbox);

    await waitFor(() => expect(checkbox.checked).toBe(false));
    expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
  });

  describe("when a repo is open", () => {
    it("loads and shows the repo's skip-hooks default and commit template", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            expect(args).toEqual({ repoPath: "/repo" });
            return { defaultSkipHooks: true };
          case "get_commit_template_path":
            return "/home/user/.gitmessage";
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByText, getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });

      expect(await waitFor(() => getByText("This Repository"))).toBeTruthy();
      await waitFor(() =>
        expect((getByLabelText("Skip hooks by default") as HTMLInputElement).checked).toBe(true),
      );
      expect((getByLabelText("Commit message template") as HTMLInputElement).value).toBe(
        "/home/user/.gitmessage",
      );
    });

    it("saves the skip-hooks default as soon as the checkbox is toggled", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "set_repo_default_skip_hooks":
            expect(args).toEqual({ repoPath: "/repo", value: true });
            return { defaultSkipHooks: true };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const checkbox = await waitFor(
        () => getByLabelText("Skip hooks by default") as HTMLInputElement,
      );

      await fireEvent.click(checkbox);

      expect(checkbox.checked).toBe(true);
    });

    it("reverts the checkbox and shows an error toast when the save fails", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "set_repo_default_skip_hooks":
            throw "disk full";
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const checkbox = await waitFor(
        () => getByLabelText("Skip hooks by default") as HTMLInputElement,
      );

      await fireEvent.click(checkbox);

      await waitFor(() => expect(checkbox.checked).toBe(false));
      expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
    });

    it("saves the commit template path, clearing it when the field is emptied", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return "/home/user/.gitmessage";
          case "set_commit_template_path":
            expect(args).toEqual({ repoPath: "/repo", path: null });
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const input = await waitFor(
        () => getByLabelText("Commit message template") as HTMLInputElement,
      );
      await waitFor(() => expect(input.value).toBe("/home/user/.gitmessage"));
      const templateForm = within(input.closest("form")!);

      await fireEvent.input(input, { target: { value: "" } });
      await fireEvent.click(templateForm.getByText("Save"));

      expect(await waitFor(() => templateForm.getByText("Saved.", { exact: false }))).toBeTruthy();
    });
  });
});
