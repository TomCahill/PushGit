// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import ConfirmDialog from "$lib/shell/ConfirmDialog.svelte";
import { makeBranchInfo } from "$lib/git/testFixtures";
import type { WorkflowConfig } from "$lib/git/types";
import WorkflowPanel from "./WorkflowPanel.svelte";

const config: WorkflowConfig = {
  main: "main",
  develop: "develop",
  featurePrefix: "feature/",
  releasePrefix: "release/",
  hotfixPrefix: "hotfix/",
  supportPrefix: null,
  versionTagPrefix: "",
};

describe("WorkflowPanel", () => {
  it("shows a placeholder when GitFlow isn't configured", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "detect_workflow":
          return null;
        case "list_branches":
          return [];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(WorkflowPanel, { props: { repoPath: "/repo", refreshKey: 0 } });

    expect(
      await findByText("GitFlow isn't set up for this repo — configure it in Settings."),
    ).toBeTruthy();
  });

  it("lists in-progress branches under their section, stripped of the prefix", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "detect_workflow":
          return config;
        case "list_branches":
          return [
            makeBranchInfo({ name: "main", isHead: true }),
            makeBranchInfo({ name: "develop" }),
            makeBranchInfo({ name: "feature/widget" }),
            makeBranchInfo({ name: "release/1.0.0" }),
          ];
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, getByText } = render(WorkflowPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText("widget")).toBeTruthy();
    expect(getByText("1.0.0")).toBeTruthy();
    expect(getByText("None in progress.")).toBeTruthy(); // Hotfix section
  });

  it("starts a feature branch via the name prompt", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "detect_workflow":
          return config;
        case "list_branches":
          return [];
        case "start_workflow_branch":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    render(ConfirmDialog);
    const { findAllByRole, findByRole } = render(WorkflowPanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const startButtons = await findAllByRole("button", { name: "Start" });
    await fireEvent.click(startButtons[0]); // Feature is the first section

    const dialog = within(await findByRole("alertdialog"));
    await fireEvent.input(dialog.getByRole("textbox"), { target: { value: "widget" } });
    await fireEvent.click(dialog.getByRole("button", { name: "OK" }));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", config, kind: "feature", name: "widget" }]),
    );
  });

  it("finishes a branch and notifies the parent on success", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "detect_workflow":
          return config;
        case "list_branches":
          return [makeBranchInfo({ name: "feature/widget" })];
        case "finish_workflow_branch":
          calls.push(args);
          return { kind: "finished" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const onConflicts = vi.fn();
    const { findByTitle } = render(WorkflowPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged, onConflicts },
    });

    await fireEvent.click(await findByTitle("Finish feature/widget"));

    await waitFor(() =>
      expect(calls).toEqual([{ repoPath: "/repo", config, kind: "feature", name: "widget" }]),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(onConflicts).not.toHaveBeenCalled();
  });

  it("routes a conflicted finish to the parent's conflict view instead of deleting the branch", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "detect_workflow":
          return config;
        case "list_branches":
          return [makeBranchInfo({ name: "feature/widget" })];
        case "finish_workflow_branch":
          return { kind: "conflicts", conflicts: ["shared.txt"] };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onConflicts = vi.fn();
    const { findByTitle } = render(WorkflowPanel, {
      props: { repoPath: "/repo", refreshKey: 0, onConflicts },
    });

    await fireEvent.click(await findByTitle("Finish feature/widget"));

    await waitFor(() => expect(onConflicts).toHaveBeenCalled());
  });
});
