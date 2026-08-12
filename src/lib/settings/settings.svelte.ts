// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// App-wide settings, persisted on the backend —
// unlike `diffViewMode.svelte.ts`'s deliberately session-only preference, the
// "max commits rendered" item is explicitly a persistent, user-visible performance setting.
// Previously stored in `localStorage`; now lives in the backend's `$XDG_CONFIG_HOME/pushgit`
// config file, so it's shared with anything else that ever reads
// config outside the webview (there's no migration — this app hasn't shipped yet).
//
// `settingsState` starts at hardcoded defaults and is populated by `loadAppConfig()`, called
// once at app startup (`+page.svelte`'s `onMount`, alongside `loadRecentRepos()`/
// `loadRepoFolders()`). Consumers (`SettingsPanel.svelte`, `CommitGraph.svelte`) read the same
// `$state` singleton either way; only the population timing changed from synchronous to async.
import {
  acknowledgeAiCloudWarning as acknowledgeAiCloudWarningCommand,
  cancelLocalAiDownload as cancelLocalAiDownloadCommand,
  clearAiApiKey as clearAiApiKeyCommand,
  downloadLocalAi as downloadLocalAiCommand,
  getAppConfig,
  getLocalAiStatus,
  hasAiApiKey as hasAiApiKeyCommand,
  setAiApiKey as setAiApiKeyCommand,
  setAiInstructions as setAiInstructionsCommand,
  setAiTransport as setAiTransportCommand,
  setAutoFetchEnabled as setAutoFetchEnabledCommand,
  setAutoFetchIntervalMinutes as setAutoFetchIntervalMinutesCommand,
  setCheckForUpdatesEnabled as setCheckForUpdatesEnabledCommand,
  setMaxCommitsRendered as setMaxCommitsRenderedCommand,
  setReduceMotion as setReduceMotionCommand,
  setShowHookOutputAlways as setShowHookOutputAlwaysCommand,
} from "$lib/git/api";
import type {
  AiSettings,
  AiTransport,
  DownloadProgress,
  EngineVariant,
  LocalAiStatus,
} from "$lib/git/types";

export const DEFAULT_MAX_COMMITS_RENDERED = 500;
export const MIN_MAX_COMMITS_RENDERED = 50;

// Mirror of `src-tauri/src/config/mod.rs`'s auto-fetch constants.
export const DEFAULT_AUTO_FETCH_INTERVAL_MINUTES = 5;
export const MIN_AUTO_FETCH_INTERVAL_MINUTES = 1;
export const MAX_AUTO_FETCH_INTERVAL_MINUTES = 60;

const DEFAULT_AI_SETTINGS: AiSettings = {
  transport: null,
  instructions: "",
  cloudWarningAcknowledged: false,
};

const DEFAULT_LOCAL_AI_STATUS: LocalAiStatus = {
  modelPresent: false,
  enginePresent: false,
  gpuDevice: null,
};

/** The saved `ManagedLocal` transport's engine variant, or the default (`"cpu"`) when no
 *  transport is saved or a different provider is active — used to know which variant's status
 *  `settingsState.localAiStatus` should reflect for `StagingPanel`'s readiness check. */
function savedEngineVariant(): EngineVariant {
  const transport = settingsState.ai.transport;
  return transport?.kind === "managedLocal" ? transport.engineVariant : "cpu";
}

interface SettingsState {
  maxCommitsRendered: number;
  reduceMotion: boolean;
  ai: AiSettings;
  /** On by default — it only talks to the remote the repo already has configured, the same
   *  one a manual Fetch click would use. */
  autoFetchEnabled: boolean;
  autoFetchIntervalMinutes: number;
  /** Whether the commit/push hook-output transcript opens immediately when a hook starts
   *  running. Off means it stays hidden (still recording) until the operation fails. */
  showHookOutputAlways: boolean;
  /** Whether `checkForUpdate()` runs on launch. Only ever queries GitHub's public releases API
   *  for this repo; never sends any identifying data. */
  checkForUpdatesEnabled: boolean;
  /** The version string of a release the user has already dismissed the update banner for. */
  dismissedUpdateVersion: string | null;
  /** Whether an AI provider API key is currently saved — the key's value itself is never
   *  read back into the frontend, see `hasAiApiKey`. */
  hasAiApiKey: boolean;
  /** Whether the managed local-AI model/engine are downloaded and verified. */
  localAiStatus: LocalAiStatus;
  /** The in-progress download's latest reported progress, or `null` when no download is
   *  running — drives the Settings panel's progress bar. */
  localAiDownloadProgress: DownloadProgress | null;
}

export const settingsState: SettingsState = $state({
  maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
  reduceMotion: false,
  ai: { ...DEFAULT_AI_SETTINGS },
  autoFetchEnabled: true,
  autoFetchIntervalMinutes: DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
  showHookOutputAlways: true,
  checkForUpdatesEnabled: true,
  dismissedUpdateVersion: null,
  hasAiApiKey: false,
  localAiStatus: { ...DEFAULT_LOCAL_AI_STATUS },
  localAiDownloadProgress: null,
});

/** Loads the backend's app config into `settingsState`. Call once at startup. Failure
 *  (including in tests that don't mock `get_app_config`) leaves `settingsState` at its
 *  hardcoded defaults rather than rejecting — this is a startup nicety, not something that
 *  should ever be able to block the app from rendering. */
export async function loadAppConfig(): Promise<void> {
  try {
    const config = await getAppConfig();
    settingsState.maxCommitsRendered = config.maxCommitsRendered;
    settingsState.reduceMotion = config.reduceMotion;
    settingsState.ai = config.ai;
    settingsState.autoFetchEnabled = config.autoFetchEnabled;
    settingsState.autoFetchIntervalMinutes = config.autoFetchIntervalMinutes;
    settingsState.showHookOutputAlways = config.showHookOutputAlways;
    settingsState.checkForUpdatesEnabled = config.checkForUpdatesEnabled;
    settingsState.dismissedUpdateVersion = config.dismissedUpdateVersion;
  } catch {
    // Keep the hardcoded defaults.
  }
  try {
    settingsState.hasAiApiKey = await hasAiApiKeyCommand();
  } catch {
    // Keep the default (false) — an unreadable keyring reads the same as "no key saved."
  }
  await refreshLocalAiStatus();
}

/** Re-reads local-AI download/readiness status from the backend for the currently *saved*
 *  transport's engine variant — called at startup and again after a download finishes (success
 *  or failure), since either can change what's on disk. Drives `StagingPanel`'s readiness
 *  check, which only cares about the saved variant, not whatever the Settings form is
 *  previewing — see `fetchLocalAiStatus` for that. */
export async function refreshLocalAiStatus(): Promise<void> {
  try {
    settingsState.localAiStatus = await getLocalAiStatus(savedEngineVariant());
  } catch {
    // Keep the previous value — an unresolvable data dir reads the same as "not downloaded."
  }
}

/** Reads local-AI status for an arbitrary `engineVariant` without touching
 *  `settingsState.localAiStatus` — used by the Settings panel to preview a drafted (not yet
 *  saved) engine-variant selection, so switching CPU/GPU in the form updates its own status
 *  line without requiring Save first or clobbering the saved variant's status elsewhere. */
export async function fetchLocalAiStatus(engineVariant: EngineVariant): Promise<LocalAiStatus> {
  return getLocalAiStatus(engineVariant);
}

/** Downloads and verifies the local-AI engine for `engineVariant` and the (shared) model,
 *  updating `localAiDownloadProgress` as chunks arrive and refreshing `localAiStatus` once it
 *  settles either way. Rethrows on failure/cancellation so the calling UI can surface the
 *  error. */
export async function downloadLocalAi(engineVariant: EngineVariant): Promise<void> {
  settingsState.localAiDownloadProgress = null;
  try {
    await downloadLocalAiCommand(engineVariant, (progress) => {
      settingsState.localAiDownloadProgress = progress;
    });
  } finally {
    settingsState.localAiDownloadProgress = null;
    await refreshLocalAiStatus();
  }
}

/** Cancels whatever local-AI download is currently in progress, if any. */
export async function cancelLocalAiDownload(): Promise<void> {
  await cancelLocalAiDownloadCommand();
}

/** Persists a new max-commits-rendered default; the backend clamps it to
 *  `MIN_MAX_COMMITS_RENDERED`, and `settingsState` is updated from its (clamped) response. */
export async function setMaxCommitsRendered(value: number): Promise<void> {
  const config = await setMaxCommitsRenderedCommand(value);
  settingsState.maxCommitsRendered = config.maxCommitsRendered;
}

/** Persists the app-wide "reduce motion" preference; `settingsState` is updated from the
 *  backend's response. */
export async function setReduceMotion(value: boolean): Promise<void> {
  const config = await setReduceMotionCommand(value);
  settingsState.reduceMotion = config.reduceMotion;
}

/** Persists whether the periodic auto-fetch timer is enabled; `settingsState` is updated from
 *  the backend's response. */
export async function setAutoFetchEnabled(value: boolean): Promise<void> {
  const config = await setAutoFetchEnabledCommand(value);
  settingsState.autoFetchEnabled = config.autoFetchEnabled;
}

/** Persists whether the launch-time update check runs at all; `settingsState` is updated from
 *  the backend's response. */
export async function setCheckForUpdatesEnabled(value: boolean): Promise<void> {
  const config = await setCheckForUpdatesEnabledCommand(value);
  settingsState.checkForUpdatesEnabled = config.checkForUpdatesEnabled;
}

/** Persists a new auto-fetch interval; the backend clamps it to
 *  `[MIN_AUTO_FETCH_INTERVAL_MINUTES, MAX_AUTO_FETCH_INTERVAL_MINUTES]`, and `settingsState` is
 *  updated from its (clamped) response. */
export async function setAutoFetchIntervalMinutes(value: number): Promise<void> {
  const config = await setAutoFetchIntervalMinutesCommand(value);
  settingsState.autoFetchIntervalMinutes = config.autoFetchIntervalMinutes;
}

/** Persists whether the hook-output transcript opens immediately or only on failure;
 *  `settingsState` is updated from the backend's response. */
export async function setShowHookOutputAlways(value: boolean): Promise<void> {
  const config = await setShowHookOutputAlwaysCommand(value);
  settingsState.showHookOutputAlways = config.showHookOutputAlways;
}

/** Persists the chosen AI transport (`null` clears it); `settingsState.ai` is updated from
 *  the backend's response, and `localAiStatus` is re-synced to whatever variant is now saved
 *  — otherwise it'd keep reflecting whichever variant was saved before this call (or the
 *  `cpu` fallback), leaving `StagingPanel`'s readiness check stale after switching providers
 *  or CPU/GPU variants. */
export async function setAiTransport(transport: AiTransport | null): Promise<void> {
  settingsState.ai = await setAiTransportCommand(transport);
  await refreshLocalAiStatus();
}

/** Persists the free-text instructions appended to every generation prompt. */
export async function setAiInstructions(instructions: string): Promise<void> {
  settingsState.ai = await setAiInstructionsCommand(instructions);
}

/** Records that the user has confirmed the one-time cloud-egress warning — called only from
 *  that confirmation dialog's "Continue" action. */
export async function acknowledgeAiCloudWarning(): Promise<void> {
  settingsState.ai = await acknowledgeAiCloudWarningCommand();
}

/** Saves the AI provider API key and refreshes `settingsState.hasAiApiKey`. */
export async function setAiApiKey(key: string): Promise<void> {
  await setAiApiKeyCommand(key);
  settingsState.hasAiApiKey = await hasAiApiKeyCommand();
}

/** Clears the stored AI provider API key and refreshes `settingsState.hasAiApiKey`. */
export async function clearAiApiKey(): Promise<void> {
  await clearAiApiKeyCommand();
  settingsState.hasAiApiKey = await hasAiApiKeyCommand();
}
