// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Drives the "a new PushGit release is available" banner. `checkNow()` is called once from
// `+page.svelte`'s `onMount`, after `loadAppConfig()` resolves — the store itself doesn't
// know about app startup timing, same split as `recentRepos.ts`.
import { checkForUpdate, dismissUpdate } from "$lib/git/api";
import type { ReleaseInfo } from "$lib/git/types";
import { settingsState } from "$lib/settings/settings.svelte";

interface UpdateCheckState {
  available: ReleaseInfo | null;
}

export const updateCheckState: UpdateCheckState = $state({
  available: null,
});

/** Checks GitHub for a newer release and populates `available` if one exists and hasn't
 *  already been dismissed. A no-op when `settingsState.checkForUpdatesEnabled` is off, or when
 *  the check itself fails (offline, GitHub down) — indistinguishable from "no update" by
 *  design, see `checkForUpdate`'s doc comment. */
export async function checkNow(): Promise<void> {
  if (!settingsState.checkForUpdatesEnabled) return;
  try {
    const release = await checkForUpdate();
    if (release && release.version !== settingsState.dismissedUpdateVersion) {
      updateCheckState.available = release;
    }
  } catch {
    // Startup nicety only — never block or error the app over this.
  }
}

/** Dismisses the current banner, persisting the dismissal so it doesn't reappear for this same
 *  release on the next launch. */
export async function dismiss(): Promise<void> {
  const release = updateCheckState.available;
  if (!release) return;
  updateCheckState.available = null;
  try {
    const config = await dismissUpdate(release.version);
    settingsState.dismissedUpdateVersion = config.dismissedUpdateVersion;
  } catch {
    // Best-effort — worst case the banner reappears next launch.
  }
}
