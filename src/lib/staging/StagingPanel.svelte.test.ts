// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import StagingPanel from "./StagingPanel.svelte";
import { makeFileDiff, makeHunk } from "$lib/git/testFixtures";

describe("StagingPanel", () => {
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

  it("copies a file's path without selecting it", async () => {
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
    const onDiffChange = vi.fn();
    const { findByRole, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onDiffChange },
    });

    await findByText("a.txt");
    await fireEvent.click(await findByRole("button", { name: "Copy path a.txt" }));

    expect(writeText).toHaveBeenCalledWith("a.txt");
    expect(onDiffChange).toHaveBeenLastCalledWith(null);
  });

  it("calls onBlame with the file's path when its Blame button is clicked", async () => {
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
    const { findByTitle } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onBlame },
    });

    await fireEvent.click(await findByTitle("Blame"));

    expect(onBlame).toHaveBeenCalledWith("a.txt");
  });

  it("stashes a single file when its Stash button is clicked, and notifies the parent", async () => {
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
    const { findByTitle, findByText } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    await findByText("Changes (1)");
    await fireEvent.click(await findByTitle("Stash this file"));

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

  it("discards a file's changes after confirmation, and notifies the parent", async () => {
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
    const onChanged = vi.fn();
    const { findByTitle, findByText, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    await findByText("Changes (1)");
    await fireEvent.click(await findByTitle("Discard changes"));
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
    const { findByTitle, findByRole } = render(StagingPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await fireEvent.click(await findByTitle("Discard changes"));
    await fireEvent.click(
      within(await findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );

    expect(await findByTitle("Discard changes")).toBeTruthy();
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
        case "diff_staged":
          return [];
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
          return [makeFileDiff({ newPath: "a.txt" })];
        case "diff_staged":
          return [];
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
});
