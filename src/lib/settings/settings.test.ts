// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import {
  DEFAULT_MAX_COMMITS_RENDERED,
  loadAppConfig,
  setMaxCommitsRendered,
  setReduceMotion,
  settingsState,
} from "./settings.svelte";

describe("settingsState", () => {
  beforeEach(() => {
    settingsState.maxCommitsRendered = DEFAULT_MAX_COMMITS_RENDERED;
    settingsState.reduceMotion = false;
  });

  it("loadAppConfig populates settingsState from the backend", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_app_config") return { maxCommitsRendered: 1000, reduceMotion: true };
      throw new Error(`unexpected command ${cmd}`);
    });

    await loadAppConfig();

    expect(settingsState.maxCommitsRendered).toBe(1000);
    expect(settingsState.reduceMotion).toBe(true);
  });

  it("setMaxCommitsRendered persists through the backend and updates settingsState from its (clamped) response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_max_commits_rendered") {
        expect(args).toEqual({ value: 10 });
        return { maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED, reduceMotion: false };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setMaxCommitsRendered(10);

    expect(settingsState.maxCommitsRendered).toBe(DEFAULT_MAX_COMMITS_RENDERED);
  });

  it("setReduceMotion persists through the backend and updates settingsState from its response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_reduce_motion") {
        expect(args).toEqual({ value: true });
        return { maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED, reduceMotion: true };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setReduceMotion(true);

    expect(settingsState.reduceMotion).toBe(true);
  });
});
