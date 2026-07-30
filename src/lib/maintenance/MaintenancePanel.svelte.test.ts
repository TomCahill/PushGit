// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import MaintenancePanel from "./MaintenancePanel.svelte";
import { toastState } from "$lib/shell/toast.svelte";

describe("MaintenancePanel", () => {
  beforeEach(() => {
    toastState.toasts = [];
  });


  it("loads and renders repo health", async () => {
    mockIPC((cmd) => {
      switch (cmd) {
        case "repo_health":
          return { looseObjectCount: 12, looseObjectSizeKib: 48, packCount: 1, packedSizeKib: 250 };
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText } = render(MaintenancePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    expect(await findByText("12 (48 KiB)")).toBeTruthy();
    expect(await findByText("1 (250 KiB)")).toBeTruthy();
  });

  it("runs git gc and refreshes health", async () => {
    let gcRan = false;
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "repo_health":
          return gcRan
            ? { looseObjectCount: 0, looseObjectSizeKib: 0, packCount: 1, packedSizeKib: 250 }
            : { looseObjectCount: 12, looseObjectSizeKib: 48, packCount: 1, packedSizeKib: 250 };
        case "run_gc":
          gcRan = true;
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { findByText, findByRole } = render(MaintenancePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    await findByText("12 (48 KiB)");
    await fireEvent.click(await findByRole("button", { name: "Run git gc" }));

    await waitFor(() => expect(calls).toEqual([{ repoPath: "/repo" }]));
    await waitFor(() => expect(toastState.toasts.map((t) => t.message)).toContain("Ran git gc."));
    expect(await findByText("0 (0 KiB)")).toBeTruthy();
  });

  it("surfaces a load error", async () => {
    mockIPC((cmd) => {
      if (cmd === "repo_health") throw new Error("not a repository");
      throw new Error(`unexpected command ${cmd}`);
    });

    const { findByRole } = render(MaintenancePanel, {
      props: { repoPath: "/repo", refreshKey: 0 },
    });

    const alert = await findByRole("alert");
    expect(alert.textContent).toContain("not a repository");
  });
});
