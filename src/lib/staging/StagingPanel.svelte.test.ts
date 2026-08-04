// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import ContextMenu from "$lib/shell/ContextMenu.svelte";
import StagingPanel from "./StagingPanel.svelte";
import { makeFileDiff, makeHunk } from "$lib/git/testFixtures";
import { settingsState } from "$lib/settings/settings.svelte";

type AiChannel = { channel: { onmessage: (chunk: { text: string }) => void } };

describe("StagingPanel", () => {
  afterEach(() => {
    settingsState.ai = { transport: null, instructions: "", cloudWarningAcknowledged: false };
    settingsState.localAiStatus = { modelPresent: false, enginePresent: false, gpuDevice: null };
  });

  it("does nothing when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called");
    });

    const { getByText } = render(StagingPanel, { props: { repoPath: "", refreshKey: 0 } });

    expect(getByText("Staged Changes (0)")).toBeTruthy();
    expect(getByText("Changes (0)")).toBeTruthy();
  });

  it("loads and renders the unstaged and staged file lists", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "unstaged.txt" })];
        case "diff_staged":
          return [makeFileDiff({ newPath: "staged.txt" })];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    expect(await findByText("unstaged.txt")).toBeTruthy();
    expect(await findByText("staged.txt")).toBeTruthy();
    expect(await findByText("Staged Changes (1)")).toBeTruthy();
    expect(await findByText("Changes (1)")).toBeTruthy();
  });

  it("shows the old and new path for a renamed or copied file", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [
            makeFileDiff({ oldPath: "old.txt", newPath: "new.txt", status: "renamed" }),
            makeFileDiff({ oldPath: "src.txt", newPath: "copy.txt", status: "copied" }),
          ];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    expect(await findByText("old.txt → new.txt")).toBeTruthy();
    expect(await findByText("src.txt → copy.txt")).toBeTruthy();
  });

  it("shows insertion/deletion counts per file and as a section total", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [
            makeFileDiff({ newPath: "a.txt", insertions: 5, deletions: 2 }),
            makeFileDiff({ newPath: "b.txt", insertions: 1, deletions: 1 }),
          ];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    await findByText("a.txt");
    // Per-file: a.txt (+5/-2) and b.txt (+1/-1).
    expect(await findByText("+5")).toBeTruthy();
    expect(await findByText("-2")).toBeTruthy();
    expect(await findByText("+1")).toBeTruthy();
    expect(await findByText("-1")).toBeTruthy();
    // Section total: 5+1 insertions, 2+1 deletions.
    expect(await findByText("+6")).toBeTruthy();
    expect(await findByText("-3")).toBeTruthy();
  });

  it("copies a file's path from its right-click menu", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    render(ContextMenu);
    const { findByRole, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.contextMenu(await findByText("a.txt"));
    await fireEvent.click(await findByRole("menuitem", { name: "Copy file path" }));

    expect(writeText).toHaveBeenCalledWith("a.txt");
  });

  it("calls onBlame with the file's path when Blame is chosen from the right-click menu", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onBlame = vi.fn();
    render(ContextMenu);
    const { findByRole, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onBlame },
    });

    await fireEvent.contextMenu(await findByText("a.txt"));
    await fireEvent.click(await findByRole("menuitem", { name: "Blame" }));

    expect(onBlame).toHaveBeenCalledWith("a.txt");
  });

  it("stashes a single file from its right-click menu, and notifies the parent", async () => {
    let stashed = false;
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return stashed ? [] : [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        case "create_stash_for_paths":
          stashed = true;
          calls.push(args);
          return "new-oid";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    render(ContextMenu);
    const { findByRole, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    await fireEvent.contextMenu(await findByText("a.txt"));
    await fireEvent.click(await findByRole("menuitem", { name: "Stash" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", message: null, paths: ["a.txt"] }]),
    );
    expect(await findByText("Changes (0)")).toBeTruthy();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("stages a whole file when its + button is clicked", async () => {
    let staged = false;
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return staged ? [] : [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return staged ? [makeFileDiff({ newPath: "a.txt" })] : [];
        case "stage_file":
          staged = true;
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByTitle, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await findByText("Changes (1)");
    await fireEvent.click(await findByTitle("Stage"));

    expect(await findByText("Staged Changes (1)")).toBeTruthy();
    expect(await findByText("Changes (0)")).toBeTruthy();
  });

  it("discards a file's changes from its right-click menu after confirmation, and notifies the parent", async () => {
    let discarded = false;
    const discardCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return discarded ? [] : [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        case "discard_file_changes":
          discarded = true;
          discardCalls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    render(ContextMenu);
    const onChanged = vi.fn();
    const { findByText, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    await fireEvent.contextMenu(await findByText("a.txt"));
    await fireEvent.click(await findByRole("menuitem", { name: "Discard changes" }));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(discardCalls).toEqual([{ repoPath: "/repo", path: "a.txt" }]));
    expect(await findByText("Changes (0)")).toBeTruthy();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("cancelling the discard confirmation makes no backend call", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    render(ContextMenu);
    const { findByText, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.contextMenu(await findByText("a.txt"));
    await fireEvent.click(await findByRole("menuitem", { name: "Discard changes" }));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );

    await fireEvent.contextMenu(await findByText("a.txt"));
    expect(await findByRole("menuitem", { name: "Discard changes" })).toBeTruthy();
  });

  it("stages every unstaged file when Stage All is clicked", async () => {
    let staged = false;
    const stagedPaths: string[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return staged
            ? []
            : [makeFileDiff({ newPath: "a.txt" }), makeFileDiff({ newPath: "b.txt" })];
        case "diff_staged":
          return staged
            ? [makeFileDiff({ newPath: "a.txt" }), makeFileDiff({ newPath: "b.txt" })]
            : [];
        case "stage_file":
          stagedPaths.push((args as { path: string }).path);
          if (stagedPaths.length === 2) staged = true;
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await findByText("Changes (2)");
    await fireEvent.click(await findByRole("button", { name: "Stage All" }));

    expect(await findByText("Staged Changes (2)")).toBeTruthy();
    expect(await findByText("Changes (0)")).toBeTruthy();
    expect(stagedPaths).toEqual(["a.txt", "b.txt"]);
  });

  it("disables Stage All when there is nothing stageable", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const button = (await findByRole("button", { name: "Stage All" })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  it("excludes conflicted files from Stage All", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [makeFileDiff({ newPath: "conflict.txt", status: "conflicted" })];
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const button = (await findByRole("button", { name: "Stage All" })) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  it("reports the selected file's diff, with a hunk-stage action, to onDiffChange", async () => {
    let hunkStaged = false;
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return hunkStaged ? [] : [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
        case "stage_hunk":
          expect(args).toMatchObject({ repoPath: "/repo", path: "a.txt" });
          hunkStaged = true;
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onDiffChange = vi.fn();
    const { findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onDiffChange },
    });

    await fireEvent.click(await findByText("a.txt"));

    await waitFor(() =>
      expect(onDiffChange).toHaveBeenLastCalledWith(
        expect.objectContaining({
          file: expect.objectContaining({ newPath: "a.txt" }),
          hunkActionLabel: "Stage hunk",
        }),
      ),
    );
    const diff = onDiffChange.mock.calls.at(-1)![0];

    diff.onHunkAction(diff.file.hunks[0]);
    await waitFor(() => expect(hunkStaged).toBe(true));
  });

  it("reports a line-stage action that only stages the given line indices", async () => {
    let linesStaged = false;
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return linesStaged
            ? []
            : [
                makeFileDiff({
                  newPath: "a.txt",
                  hunks: [
                    makeHunk({
                      lines: [
                        { origin: "addition", content: "line a\n", oldLineno: null, newLineno: 1 },
                        { origin: "addition", content: "line b\n", oldLineno: null, newLineno: 2 },
                      ],
                    }),
                  ],
                }),
              ];
        case "diff_staged":
          return [];
        case "stage_lines":
          expect(args).toMatchObject({ repoPath: "/repo", path: "a.txt", lineIndices: [0] });
          linesStaged = true;
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onDiffChange = vi.fn();
    const { findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onDiffChange },
    });

    await fireEvent.click(await findByText("a.txt"));

    await waitFor(() =>
      expect(onDiffChange).toHaveBeenLastCalledWith(
        expect.objectContaining({ lineActionLabel: "Stage" }),
      ),
    );
    const diff = onDiffChange.mock.calls.at(-1)![0];

    diff.onLineAction(diff.file.hunks[0], [0]);
    await waitFor(() => expect(linesStaged).toBe(true));
  });

  it("commits staged changes and notifies the parent", async () => {
    const commitCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "commit":
          commitCalls.push(args);
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "fix the bug" } });
    await fireEvent.click(getByRole("button", { name: "Commit" }));

    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(commitCalls).toEqual([
      { repoPath: "/repo", message: "fix the bug", amend: false, skipHooks: false },
    ]);
  });

  it("passes skipHooks through when the checkbox is checked, and resets it after committing", async () => {
    const commitCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "commit":
          commitCalls.push(args);
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "fix the bug" } });
    const skipHooksCheckbox = getByRole("checkbox", { name: "Skip hooks" }) as HTMLInputElement;
    await fireEvent.click(skipHooksCheckbox);
    await fireEvent.click(getByRole("button", { name: "Commit" }));

    await waitFor(() => expect(commitCalls).toHaveLength(1));
    expect(commitCalls).toEqual([
      { repoPath: "/repo", message: "fix the bug", amend: false, skipHooks: true },
    ]);
    await waitFor(() => expect(skipHooksCheckbox.checked).toBe(false));
  });

  it("initializes the skip-hooks checkbox from the repo's default, and resets to it after committing", async () => {
    const commitCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "get_repo_config":
          return { defaultSkipHooks: true };
        case "commit":
          commitCalls.push(args);
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const skipHooksCheckbox = await waitFor(
      () => getByRole("checkbox", { name: "Skip hooks" }) as HTMLInputElement,
    );
    await waitFor(() => expect(skipHooksCheckbox.checked).toBe(true));

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "fix the bug" } });
    await fireEvent.click(getByRole("button", { name: "Commit" }));

    await waitFor(() => expect(commitCalls).toHaveLength(1));
    expect(commitCalls).toEqual([
      { repoPath: "/repo", message: "fix the bug", amend: false, skipHooks: true },
    ]);
    await waitFor(() => expect(skipHooksCheckbox.checked).toBe(true));
  });

  it("doesn't let a slow-loading repo default clobber a skip-hooks checkbox the user already toggled", async () => {
    let resolveRepoConfig!: (config: { defaultSkipHooks: boolean }) => void;
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [];
        case "get_repo_config":
          return new Promise((resolve) => {
            resolveRepoConfig = resolve;
          });
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });
    const skipHooksCheckbox = getByRole("checkbox", { name: "Skip hooks" }) as HTMLInputElement;

    await fireEvent.click(skipHooksCheckbox);
    expect(skipHooksCheckbox.checked).toBe(true);

    resolveRepoConfig({ defaultSkipHooks: false });
    await Promise.resolve();
    await Promise.resolve();

    expect(skipHooksCheckbox.checked).toBe(true);
  });

  it("surfaces a pre-commit hook rejection as the commit error", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "commit":
          throw "git error: `pre-commit` hook rejected the commit:\nlint failed";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "fix the bug" } });
    await fireEvent.click(getByRole("button", { name: "Commit" }));

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("pre-commit` hook rejected the commit");
  });

  it("caps the commit title at 72 characters and rejects an empty title", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    const commitButton = getByRole("button", { name: "Commit" }) as HTMLButtonElement;
    expect(titleInput.maxLength).toBe(72);
    expect(commitButton.disabled).toBe(true);

    await fireEvent.input(titleInput, { target: { value: "a title" } });
    expect(commitButton.disabled).toBe(false);
  });

  it("keeps Commit disabled with a title but nothing staged, unless amending", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        case "head_commit_message":
          return "previous commit message";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, findByRole, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "a title" } });

    const commitButton = getByRole("button", { name: "Commit" }) as HTMLButtonElement;
    expect(commitButton.disabled).toBe(true);
    expect(commitButton.title).toBe("Stage changes first");

    // Toggling amend with a title already typed would trigger the overwrite-confirmation
    // dialog (covered separately below) — clear it first so this test stays focused on the
    // staged-files gating.
    await fireEvent.input(titleInput, { target: { value: "" } });
    await fireEvent.click(getByRole("checkbox", { name: "Amend last commit" }));
    await waitFor(() => expect(titleInput.value).toBe("previous commit message"));
    const amendButton = await findByRole("button", { name: "Amend" });
    expect((amendButton as HTMLButtonElement).disabled).toBe(false);
  });

  it("combines the title and optional description into the commit message", async () => {
    const commitCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "commit":
          commitCalls.push(args);
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    const descriptionInput = (await findByLabelText(
      "Description (optional)",
    )) as HTMLTextAreaElement;
    await fireEvent.input(titleInput, { target: { value: "Fix the bug" } });
    await fireEvent.input(descriptionInput, { target: { value: "More detail here." } });
    await fireEvent.click(getByRole("button", { name: "Commit" }));

    await waitFor(() => expect(commitCalls).toHaveLength(1));
    expect(commitCalls).toEqual([
      {
        repoPath: "/repo",
        message: "Fix the bug\n\nMore detail here.",
        amend: false,
        skipHooks: false,
      },
    ]);
  });

  it("pre-fills the title and description with HEAD's when amend is toggled on", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        case "head_commit_message":
          return "previous commit message\n\nWith a body line.";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(getByRole("checkbox", { name: "Amend last commit" }));

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    const descriptionInput = (await findByLabelText(
      "Description (optional)",
    )) as HTMLTextAreaElement;
    await waitFor(() => expect(titleInput.value).toBe("previous commit message"));
    expect(descriptionInput.value).toBe("With a body line.");
  });

  it("asks for confirmation before amend overwrites an already-typed commit message", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        case "head_commit_message":
          return "previous commit message";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findByLabelText, findByRole, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "my draft title" } });

    await fireEvent.click(getByRole("checkbox", { name: "Amend last commit" }));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
    );

    await waitFor(() => expect(titleInput.value).toBe("previous commit message"));
  });

  it("cancelling the amend confirmation leaves the draft untouched and amend off", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        default:
          throw new Error(`unexpected command ${cmd} — cancelling should make no other backend call`);
      }
    });

    render(ConfirmDialog);
    const { findByLabelText, findByRole, getByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "my draft title" } });

    await fireEvent.click(getByRole("checkbox", { name: "Amend last commit" }));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );

    expect(titleInput.value).toBe("my draft title");
    expect((getByRole("checkbox", { name: "Amend last commit" }) as HTMLInputElement).checked).toBe(
      false,
    );
  });

  it("pre-fills the title and description from commit.template on open", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        case "commit_message_template":
          return "Template title\n\nTemplate body.";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    const descriptionInput = (await findByLabelText(
      "Description (optional)",
    )) as HTMLTextAreaElement;
    await waitFor(() => expect(titleInput.value).toBe("Template title"));
    expect(descriptionInput.value).toBe("Template body.");
  });

  it("never overwrites a message the user already started typing with the template", async () => {
    let resolveTemplate = (_value: string) => {};
    const templateGate = new Promise<string>((resolve) => {
      resolveTemplate = resolve;
    });
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
        case "diff_staged":
          return [];
        case "commit_message_template":
          return templateGate;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await fireEvent.input(titleInput, { target: { value: "user typed this first" } });

    resolveTemplate("Template title");
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(titleInput.value).toBe("user typed this first");
  });

  it("re-applies the template after a successful commit clears the box", async () => {
    let templateCalls = 0;
    mockIPC((cmd) => {
      switch (cmd) {
        case "diff_unstaged":
          return [];
        case "diff_staged":
          return [makeFileDiff({ newPath: "a.txt" })];
        case "commit_message_template":
          templateCalls += 1;
          return "Template title";
        case "commit":
          return "deadbeef";
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByLabelText, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
    await waitFor(() => expect(titleInput.value).toBe("Template title"));

    await fireEvent.click(await findByRole("button", { name: "Commit" }));

    await waitFor(() => expect(templateCalls).toBe(2));
    await waitFor(() => expect(titleInput.value).toBe("Template title"));
  });

  it("surfaces a backend error instead of crashing", async () => {
    mockIPC((cmd) => {
      if (cmd === "diff_unstaged") throw "git error: not a repository";
      if (cmd === "diff_staged") return [];
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });

  describe("Generate with AI", () => {
    const LOCAL_TRANSPORT = {
      kind: "openAiCompatible" as const,
      baseUrl: "http://localhost:11434/v1",
      model: "llama3.1",
    };
    const CLOUD_TRANSPORT = {
      kind: "anthropic" as const,
      baseUrl: "https://api.anthropic.com",
      model: "claude-sonnet-5",
    };

    it("is disabled with a 'stage changes' reason when nothing is staged", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged" || cmd === "diff_staged") return [];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle("Stage changes first")) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
    });

    it("is disabled pointing at Settings when no provider is configured", async () => {
      settingsState.ai.transport = null;
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle(
        "Configure an AI provider in Settings first",
      )) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
    });

    it("is disabled pointing at Settings when the transport is half-configured", async () => {
      settingsState.ai.transport = { kind: "openAiCompatible", baseUrl: "", model: "" };
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle(
        "Configure an AI provider in Settings first",
      )) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
    });

    it("is disabled pointing at Settings when the managed local engine isn't downloaded yet", async () => {
      settingsState.ai.transport = { kind: "managedLocal", engineVariant: "cpu" };
      settingsState.localAiStatus = { modelPresent: false, enginePresent: false, gpuDevice: null };
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle(
        "Download the local AI engine in Settings first",
      )) as HTMLButtonElement;
      expect(button.disabled).toBe(true);
    });

    it("is enabled for the managed local transport once the engine and model are downloaded", async () => {
      settingsState.ai.transport = { kind: "managedLocal", engineVariant: "cpu" };
      settingsState.localAiStatus = { modelPresent: true, enginePresent: true, gpuDevice: null };
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle(
        "Generate a commit message from the staged diff",
      )) as HTMLButtonElement;
      expect(button.disabled).toBe(false);
    });

    it("is enabled once files are staged and a provider is fully configured", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle } = render(StagingPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

      const button = (await findByTitle(
        "Generate a commit message from the staged diff",
      )) as HTMLButtonElement;
      expect(button.disabled).toBe(false);
    });

    it("streams generated text into the title/description live, for a local transport with no cloud-warning dialog", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "feat: add thing\n\nLonger body." });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      const descriptionInput = (await findByLabelText(
        "Description (optional)",
      )) as HTMLTextAreaElement;
      await waitFor(() => expect(titleInput.value).toBe("feat: add thing"));
      expect(descriptionInput.value).toBe("Longer body.");
    });

    it("streams generated text for the managed local transport with no cloud-warning dialog", async () => {
      settingsState.ai.transport = { kind: "managedLocal", engineVariant: "cpu" };
      settingsState.localAiStatus = { modelPresent: true, enginePresent: true, gpuDevice: null };
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "feat: from local engine" });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await waitFor(() => expect(titleInput.value).toBe("feat: from local engine"));
    });

    it("strips a markdown code fence a weaker model wraps its output in", async () => {
      settingsState.ai.transport = { kind: "managedLocal", engineVariant: "cpu" };
      settingsState.localAiStatus = { modelPresent: true, enginePresent: true, gpuDevice: null };
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({
            text: "```plaintext\nfeat(ai): add local engine support\n\n- did a thing\n```",
          });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      const descriptionInput = (await findByLabelText(
        "Description (optional)",
      )) as HTMLTextAreaElement;
      await waitFor(() => expect(titleInput.value).toBe("feat(ai): add local engine support"));
      expect(descriptionInput.value).toBe("- did a thing");
    });

    it("freezes at 4 bullets and cancels generation once the model tries to write a 5th", async () => {
      settingsState.ai.transport = { kind: "managedLocal", engineVariant: "cpu" };
      settingsState.localAiStatus = { modelPresent: true, enginePresent: true, gpuDevice: null };
      const cancelCalls: string[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "cancel_ai_generation") {
          cancelCalls.push(cmd);
          return null;
        }
        if (cmd === "generate_commit_message") {
          const send = (text: string) => (args as AiChannel).channel.onmessage({ text });
          send("feat(ai): add local engine support\n\n");
          send("- one\n- two\n- three\n- four\n");
          send("- five\n- six\n"); // beyond the cap — must never be shown
          throw "operation cancelled"; // the cap's own cancel call resolves the invoke this way
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const descriptionInput = (await findByLabelText(
        "Description (optional)",
      )) as HTMLTextAreaElement;
      await waitFor(() => expect(descriptionInput.value).toBe("- one\n- two\n- three\n- four"));
      expect(descriptionInput.value).not.toContain("five");
      expect(cancelCalls).toEqual(["cancel_ai_generation"]);
    });

    it("shows a one-time cloud-egress confirmation for a non-local transport, and confirming persists the acknowledgement", async () => {
      settingsState.ai.transport = CLOUD_TRANSPORT;
      const ackCalls: number[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "acknowledge_ai_cloud_warning") {
          ackCalls.push(1);
          return { transport: CLOUD_TRANSPORT, instructions: "", cloudWarningAcknowledged: true };
        }
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "feat: ok" });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      render(ConfirmDialog);
      const { findByTitle, findByRole, findByLabelText } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));
      const dialog = await findByRole("alertdialog");
      expect(dialog.textContent).toContain("https://api.anthropic.com");
      await fireEvent.click(within(dialog).getByRole("button", { name: "OK" }));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await waitFor(() => expect(titleInput.value).toBe("feat: ok"));
      expect(ackCalls).toEqual([1]);
      expect(settingsState.ai.cloudWarningAcknowledged).toBe(true);
    });

    it("declining the cloud-egress confirmation aborts generation without persisting acknowledgement", async () => {
      settingsState.ai.transport = CLOUD_TRANSPORT;
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd} — declining should make no other backend call`);
      });

      render(ConfirmDialog);
      const { findByTitle, findByRole } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));
      const dialog = await findByRole("alertdialog");
      await fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

      expect(settingsState.ai.cloudWarningAcknowledged).toBe(false);
    });

    it("does not re-show the cloud-egress dialog once already acknowledged", async () => {
      settingsState.ai.transport = CLOUD_TRANSPORT;
      settingsState.ai.cloudWarningAcknowledged = true;
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "feat: ok" });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText, queryByRole } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await waitFor(() => expect(titleInput.value).toBe("feat: ok"));
      expect(queryByRole("alertdialog")).toBeNull();
    });

    it("asks for confirmation before replacing an already-typed commit message", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      mockIPC((cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "feat: replaced" });
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      render(ConfirmDialog);
      const { findByTitle, findByLabelText, findByRole } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await fireEvent.input(titleInput, { target: { value: "my draft title" } });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));
      await fireEvent.click(
        within(await findByRole("alertdialog")).getByRole("button", { name: "OK" }),
      );

      await waitFor(() => expect(titleInput.value).toBe("feat: replaced"));
    });

    it("cancelling the replace confirmation leaves the existing draft untouched", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      mockIPC((cmd) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        throw new Error(`unexpected command ${cmd} — cancelling should make no other backend call`);
      });

      render(ConfirmDialog);
      const { findByTitle, findByLabelText, findByRole } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await fireEvent.input(titleInput, { target: { value: "my draft title" } });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));
      await fireEvent.click(
        within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
      );

      expect(titleInput.value).toBe("my draft title");
    });

    it("shows a stop affordance while streaming and cancels via cancel_ai_generation without showing an error", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      let rejectGenerate: (reason: unknown) => void = () => {};
      const gate = new Promise<void>((_resolve, reject) => {
        rejectGenerate = reject;
      });
      const cancelCalls: unknown[] = [];

      mockIPC(async (cmd, args) => {
        if (cmd === "diff_unstaged") return [];
        if (cmd === "diff_staged") return [makeFileDiff({ newPath: "a.txt" })];
        if (cmd === "generate_commit_message") {
          (args as AiChannel).channel.onmessage({ text: "partial" });
          await gate;
          return null;
        }
        if (cmd === "cancel_ai_generation") {
          cancelCalls.push(args);
          rejectGenerate("operation cancelled");
          return null;
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { findByTitle, findByLabelText, queryByRole } = render(StagingPanel, {
        props: { repoPath: "/repo", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));

      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await waitFor(() => expect(titleInput.value).toBe("partial"));

      const stopButton = await findByTitle("Stop generating");
      await fireEvent.click(stopButton);

      await waitFor(() => expect(cancelCalls).toEqual([{ repoPath: "/repo" }]));
      expect(titleInput.value).toBe("partial"); // partial text left in place, not cleared
      expect(queryByRole("alert")).toBeNull();
    });

    it("cancels an in-flight generation when repoPath changes, leaving partial text in place", async () => {
      settingsState.ai.transport = LOCAL_TRANSPORT;
      let rejectGenerate: (reason: unknown) => void = () => {};
      const gate = new Promise<void>((_resolve, reject) => {
        rejectGenerate = reject;
      });
      const cancelCalls: unknown[] = [];

      mockIPC(async (cmd, args) => {
        switch (cmd) {
          case "diff_unstaged":
            return [];
          case "diff_staged":
            return [makeFileDiff({ newPath: "a.txt" })];
          case "commit_message_template":
            return null;
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "generate_commit_message":
            (args as AiChannel).channel.onmessage({ text: "partial from repo a" });
            await gate;
            return null;
          case "cancel_ai_generation":
            cancelCalls.push(args);
            rejectGenerate("operation cancelled");
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByTitle, findByLabelText, rerender } = render(StagingPanel, {
        props: { repoPath: "/repo-a", refreshKey: 0 },
      });

      await fireEvent.click(await findByTitle("Generate a commit message from the staged diff"));
      const titleInput = (await findByLabelText("Commit title")) as HTMLInputElement;
      await waitFor(() => expect(titleInput.value).toBe("partial from repo a"));

      await rerender({ repoPath: "/repo-b", refreshKey: 0 });

      await waitFor(() => expect(cancelCalls).toEqual([{ repoPath: "/repo-a" }]));
      expect(titleInput.value).toBe("partial from repo a");
    });
  });
});
