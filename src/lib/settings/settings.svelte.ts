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
  clearAiApiKey as clearAiApiKeyCommand,
  getAppConfig,
  hasAiApiKey as hasAiApiKeyCommand,
  setAiApiKey as setAiApiKeyCommand,
  setAiInstructions as setAiInstructionsCommand,
  setAiTransport as setAiTransportCommand,
  setMaxCommitsRendered as setMaxCommitsRenderedCommand,
  setReduceMotion as setReduceMotionCommand,
} from "$lib/git/api";
import type { AiSettings, AiTransport } from "$lib/git/types";

export const DEFAULT_MAX_COMMITS_RENDERED = 500;
export const MIN_MAX_COMMITS_RENDERED = 50;

const DEFAULT_AI_SETTINGS: AiSettings = {
  transport: null,
  instructions: "",
  cloudWarningAcknowledged: false,
};

interface SettingsState {
  maxCommitsRendered: number;
  reduceMotion: boolean;
  ai: AiSettings;
  /** Whether an AI provider API key is currently saved — the key's value itself is never
   *  read back into the frontend, see `hasAiApiKey`. */
  hasAiApiKey: boolean;
}

export const settingsState: SettingsState = $state({
  maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
  reduceMotion: false,
  ai: { ...DEFAULT_AI_SETTINGS },
  hasAiApiKey: false,
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
  } catch {
    // Keep the hardcoded defaults.
  }
  try {
    settingsState.hasAiApiKey = await hasAiApiKeyCommand();
  } catch {
    // Keep the default (false) — an unreadable keyring reads the same as "no key saved."
  }
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

/** Persists the chosen AI transport (`null` clears it); `settingsState.ai` is updated from
 *  the backend's response. */
export async function setAiTransport(transport: AiTransport | null): Promise<void> {
  settingsState.ai = await setAiTransportCommand(transport);
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
