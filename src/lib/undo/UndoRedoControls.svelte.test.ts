// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import UndoRedoControls from "./UndoRedoControls.svelte";

describe("UndoRedoControls", () => {
  it("disables both buttons when there's nothing to undo or redo", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "undo_redo_status":
          return { canUndo: false, undoLabel: null, canRedo: false, redoLabel: null };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole } = render(UndoRedoControls, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const undoButton = (await findByRole("button", {
      name: "Undo last operation",
    })) as HTMLButtonElement;
    const redoButton = (await findByRole("button", {
      name: "Redo last undone operation",
    })) as HTMLButtonElement;

    await waitFor(() => expect(undoButton.disabled).toBe(true));
    expect(redoButton.disabled).toBe(true);
  });

  it("enables Undo with the operation's label as a tooltip, and calls undo_last_operation on click", async () => {
    const undoCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "undo_redo_status":
          return {
            canUndo: true,
            undoLabel: "Delete branch 'feature'",
            canRedo: false,
            redoLabel: null,
          };
        case "undo_last_operation":
          undoCalls.push(args);
          return { label: "Delete branch 'feature'" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const onChanged = vi.fn();
    const { findByRole } = render(UndoRedoControls, {
      props: { repoPath: "/repo", refreshKey: 0, onChanged },
    });

    const undoButton = (await findByRole("button", {
      name: "Undo last operation",
    })) as HTMLButtonElement;
    await waitFor(() => expect(undoButton.disabled).toBe(false));
    expect(undoButton.title).toBe("Undo: Delete branch 'feature'");

    await fireEvent.click(undoButton);

    await waitFor(() => expect(undoCalls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("enables Redo and calls redo_last_operation on click", async () => {
    const redoCalls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "undo_redo_status":
          return {
            canUndo: false,
            undoLabel: null,
            canRedo: true,
            redoLabel: "Delete branch 'feature'",
          };
        case "redo_last_operation":
          redoCalls.push(args);
          return { label: "Delete branch 'feature'" };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole } = render(UndoRedoControls, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const redoButton = (await findByRole("button", {
      name: "Redo last undone operation",
    })) as HTMLButtonElement;
    await waitFor(() => expect(redoButton.disabled).toBe(false));

    await fireEvent.click(redoButton);

    await waitFor(() => expect(redoCalls).toEqual([{ repoPath: "/repo" }]));
  });

  it("shows an error when undo fails", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "undo_redo_status":
          return { canUndo: true, undoLabel: "Commit 'oops'", canRedo: false, redoLabel: null };
        case "undo_last_operation":
          throw new Error("nothing to undo");
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByRole, findByRole: findAlert } = render(UndoRedoControls, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const undoButton = (await findByRole("button", {
      name: "Undo last operation",
    })) as HTMLButtonElement;
    await waitFor(() => expect(undoButton.disabled).toBe(false));
    await fireEvent.click(undoButton);

    expect(await findAlert("alert")).toBeTruthy();
  });
});
