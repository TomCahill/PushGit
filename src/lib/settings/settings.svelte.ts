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
  getAppConfig,
  setMaxCommitsRendered as setMaxCommitsRenderedCommand,
  setReduceMotion as setReduceMotionCommand,
} from "$lib/git/api";

export const DEFAULT_MAX_COMMITS_RENDERED = 500;
export const MIN_MAX_COMMITS_RENDERED = 50;

interface SettingsState {
  maxCommitsRendered: number;
  reduceMotion: boolean;
}

export const settingsState: SettingsState = $state({
  maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
  reduceMotion: false,
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
  } catch {
    // Keep the hardcoded defaults.
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
