// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import { settingsState } from "$lib/settings/settings.svelte";
import { checkNow, dismiss, updateCheckState } from "./updateCheck.svelte";

const RELEASE = {
  version: "1.3.0",
  url: "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0",
  publishedAt: "2026-08-01T00:00:00Z",
};

describe("updateCheck", () => {
  beforeEach(() => {
    updateCheckState.available = null;
    settingsState.checkForUpdatesEnabled = true;
    settingsState.dismissedUpdateVersion = null;
  });

  it("populates available when the backend reports a newer release", async () => {
    mockIPC((cmd) => {
      if (cmd === "check_for_update") return RELEASE;
      throw new Error(`unexpected command ${cmd}`);
    });

    await checkNow();

    expect(updateCheckState.available).toEqual(RELEASE);
  });

  it("does not populate available for a release already dismissed", async () => {
    settingsState.dismissedUpdateVersion = "1.3.0";
    mockIPC((cmd) => {
      if (cmd === "check_for_update") return RELEASE;
      throw new Error(`unexpected command ${cmd}`);
    });

    await checkNow();

    expect(updateCheckState.available).toBeNull();
  });

  it("does not call the backend when check-for-updates is disabled", async () => {
    settingsState.checkForUpdatesEnabled = false;
    mockIPC((cmd) => {
      throw new Error(`unexpected command ${cmd}`);
    });

    await checkNow();

    expect(updateCheckState.available).toBeNull();
  });

  it("stays null when the check itself fails", async () => {
    mockIPC(() => {
      throw new Error("network error");
    });

    await checkNow();

    expect(updateCheckState.available).toBeNull();
  });

  it("dismiss clears available and persists the dismissal", async () => {
    updateCheckState.available = RELEASE;
    mockIPC((cmd, args) => {
      if (cmd === "dismiss_update") {
        expect(args).toEqual({ version: "1.3.0" });
        return { dismissedUpdateVersion: "1.3.0" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await dismiss();

    expect(updateCheckState.available).toBeNull();
    expect(settingsState.dismissedUpdateVersion).toBe("1.3.0");
  });
});
