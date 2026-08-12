// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import {
  acknowledgeAiCloudWarning,
  clearAiApiKey,
  DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
  DEFAULT_MAX_COMMITS_RENDERED,
  loadAppConfig,
  setAiApiKey,
  setAiInstructions,
  setAiTransport,
  setAutoFetchEnabled,
  setAutoFetchIntervalMinutes,
  setCheckForUpdatesEnabled,
  setMaxCommitsRendered,
  setReduceMotion,
  setShowHookOutputAlways,
  settingsState,
} from "./settings.svelte";
import type { AiSettings } from "$lib/git/types";

const DEFAULT_AI_SETTINGS: AiSettings = {
  transport: null,
  instructions: "",
  cloudWarningAcknowledged: false,
};

describe("settingsState", () => {
  beforeEach(() => {
    settingsState.maxCommitsRendered = DEFAULT_MAX_COMMITS_RENDERED;
    settingsState.reduceMotion = false;
    settingsState.ai = { ...DEFAULT_AI_SETTINGS };
    settingsState.autoFetchEnabled = false;
    settingsState.autoFetchIntervalMinutes = DEFAULT_AUTO_FETCH_INTERVAL_MINUTES;
    settingsState.showHookOutputAlways = true;
    settingsState.checkForUpdatesEnabled = true;
    settingsState.dismissedUpdateVersion = null;
    settingsState.hasAiApiKey = false;
  });

  it("loadAppConfig populates settingsState from the backend", async () => {
    const ai: AiSettings = {
      transport: { kind: "openAiCompatible", baseUrl: "http://localhost:11434/v1", model: "llama3.1" },
      instructions: "Use Conventional Commits.",
      cloudWarningAcknowledged: true,
    };
    mockIPC((cmd) => {
      if (cmd === "get_app_config") {
        return {
          maxCommitsRendered: 1000,
          reduceMotion: true,
          ai,
          autoFetchEnabled: true,
          autoFetchIntervalMinutes: 15,
          showHookOutputAlways: false,
          checkForUpdatesEnabled: false,
          dismissedUpdateVersion: "1.2.0",
        };
      }
      if (cmd === "has_ai_api_key") return true;
      throw new Error(`unexpected command ${cmd}`);
    });

    await loadAppConfig();

    expect(settingsState.maxCommitsRendered).toBe(1000);
    expect(settingsState.reduceMotion).toBe(true);
    expect(settingsState.ai).toEqual(ai);
    expect(settingsState.autoFetchEnabled).toBe(true);
    expect(settingsState.autoFetchIntervalMinutes).toBe(15);
    expect(settingsState.showHookOutputAlways).toBe(false);
    expect(settingsState.checkForUpdatesEnabled).toBe(false);
    expect(settingsState.dismissedUpdateVersion).toBe("1.2.0");
    expect(settingsState.hasAiApiKey).toBe(true);
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

  it("setAutoFetchEnabled persists through the backend and updates settingsState from its response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_auto_fetch_enabled") {
        expect(args).toEqual({ value: true });
        return {
          maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
          reduceMotion: false,
          autoFetchEnabled: true,
          autoFetchIntervalMinutes: DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAutoFetchEnabled(true);

    expect(settingsState.autoFetchEnabled).toBe(true);
  });

  it("setAutoFetchIntervalMinutes persists through the backend and updates settingsState from its (clamped) response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_auto_fetch_interval_minutes") {
        expect(args).toEqual({ value: 999 });
        return {
          maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
          reduceMotion: false,
          autoFetchEnabled: false,
          autoFetchIntervalMinutes: 60,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAutoFetchIntervalMinutes(999);

    expect(settingsState.autoFetchIntervalMinutes).toBe(60);
  });

  it("setShowHookOutputAlways persists through the backend and updates settingsState from its response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_show_hook_output_always") {
        expect(args).toEqual({ value: false });
        return {
          maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
          reduceMotion: false,
          autoFetchEnabled: false,
          autoFetchIntervalMinutes: DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
          showHookOutputAlways: false,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setShowHookOutputAlways(false);

    expect(settingsState.showHookOutputAlways).toBe(false);
  });

  it("setCheckForUpdatesEnabled persists through the backend and updates settingsState from its response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_check_for_updates_enabled") {
        expect(args).toEqual({ value: false });
        return {
          maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
          reduceMotion: false,
          autoFetchEnabled: false,
          autoFetchIntervalMinutes: DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
          checkForUpdatesEnabled: false,
        };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setCheckForUpdatesEnabled(false);

    expect(settingsState.checkForUpdatesEnabled).toBe(false);
  });

  it("setAiTransport persists through the backend and updates settingsState.ai from its response", async () => {
    const transport = { kind: "anthropic" as const, baseUrl: "https://api.anthropic.com", model: "claude-sonnet-5" };
    mockIPC((cmd, args) => {
      if (cmd === "set_ai_transport") {
        expect(args).toEqual({ transport });
        return { transport, instructions: "", cloudWarningAcknowledged: false };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAiTransport(transport);

    expect(settingsState.ai.transport).toEqual(transport);
  });

  it("setAiTransport refreshes localAiStatus for the newly saved engine variant", async () => {
    // Regression test: switching to (or between variants of) managedLocal used to leave
    // `localAiStatus` stuck on whatever variant was saved before, so `StagingPanel`'s
    // readiness check kept reporting "not downloaded" for an engine that was actually ready.
    settingsState.localAiStatus = { modelPresent: false, enginePresent: false, gpuDevice: null };
    const transport = { kind: "managedLocal" as const, engineVariant: "vulkan" as const };
    mockIPC((cmd, args) => {
      if (cmd === "set_ai_transport") {
        expect(args).toEqual({ transport });
        return { transport, instructions: "", cloudWarningAcknowledged: false };
      }
      if (cmd === "get_local_ai_status") {
        expect(args).toEqual({ engineVariant: "vulkan" });
        return { modelPresent: true, enginePresent: true, gpuDevice: "Radeon RX 7900" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAiTransport(transport);

    expect(settingsState.localAiStatus).toEqual({
      modelPresent: true,
      enginePresent: true,
      gpuDevice: "Radeon RX 7900",
    });
  });

  it("setAiInstructions persists through the backend and updates settingsState.ai from its response", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_ai_instructions") {
        expect(args).toEqual({ instructions: "Use Conventional Commits." });
        return { transport: null, instructions: "Use Conventional Commits.", cloudWarningAcknowledged: false };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAiInstructions("Use Conventional Commits.");

    expect(settingsState.ai.instructions).toBe("Use Conventional Commits.");
  });

  it("acknowledgeAiCloudWarning persists through the backend and updates settingsState.ai from its response", async () => {
    mockIPC((cmd) => {
      if (cmd === "acknowledge_ai_cloud_warning") {
        return { transport: null, instructions: "", cloudWarningAcknowledged: true };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    await acknowledgeAiCloudWarning();

    expect(settingsState.ai.cloudWarningAcknowledged).toBe(true);
  });

  it("setAiApiKey saves the key and refreshes settingsState.hasAiApiKey", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_ai_api_key") {
        expect(args).toEqual({ key: "sk-test" });
        return null;
      }
      if (cmd === "has_ai_api_key") return true;
      throw new Error(`unexpected command ${cmd}`);
    });

    await setAiApiKey("sk-test");

    expect(settingsState.hasAiApiKey).toBe(true);
  });

  it("clearAiApiKey clears the key and refreshes settingsState.hasAiApiKey", async () => {
    settingsState.hasAiApiKey = true;
    mockIPC((cmd) => {
      if (cmd === "clear_ai_api_key") return null;
      if (cmd === "has_ai_api_key") return false;
      throw new Error(`unexpected command ${cmd}`);
    });

    await clearAiApiKey();

    expect(settingsState.hasAiApiKey).toBe(false);
  });
});
