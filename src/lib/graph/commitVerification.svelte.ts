// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Per-repo cache of `verify_commits` results, keyed by oid — a commit's signature never
// changes once made, so an oid never needs re-checking once verified. `CommitGraph.svelte`
// calls `requestVerification` once per fetched page with only that page's
// `hasSignature: true` oids, never a whole-history scan, keeping this bounded to what's
// actually on screen rather than eagerly verifying every commit in a repo's history.

import { verifyCommits } from "$lib/git/api";
import type { VerificationStatus } from "$lib/git/types";

interface CommitVerificationState {
  repoPath: string;
  statuses: Record<string, VerificationStatus>;
}

// A plain object under `$state` gets Svelte 5's deep-proxy reactivity for free; a `Map`
// would need `svelte/reactivity`'s `SvelteMap` for the same guarantee, which isn't used
// elsewhere in this codebase — an object keyed by oid string is the simpler fit here.
const state: CommitVerificationState = $state({ repoPath: "", statuses: {} });

// In-flight oids, so a fast re-render (e.g. scrolling back and forth across a page
// boundary) can't fire a second `verify_commits` call for the same oid while the first is
// still pending. Never rendered directly, so it doesn't need to be part of `$state`.
const pendingOids = new Set<string>();

export function verificationStatus(oid: string): VerificationStatus | undefined {
  return state.statuses[oid];
}

/** Clears the cache on a real repo switch — a no-op if `repoPath` hasn't actually changed,
 *  so calling this on every page fetch (not just repo open) is cheap and safe. */
export function resetCommitVerification(repoPath: string): void {
  if (state.repoPath === repoPath) return;
  state.repoPath = repoPath;
  state.statuses = {};
  pendingOids.clear();
}

/** Batches `verify_commits` for every oid in `oids` not already cached or already in
 *  flight. Failures are swallowed — a commit simply stays unbadged rather than surfacing a
 *  toast for what's a purely informational, best-effort check. */
export async function requestVerification(repoPath: string, oids: string[]): Promise<void> {
  const toFetch = oids.filter((oid) => !(oid in state.statuses) && !pendingOids.has(oid));
  if (toFetch.length === 0) return;
  for (const oid of toFetch) pendingOids.add(oid);
  try {
    const results = await verifyCommits(repoPath, toFetch);
    state.statuses = { ...state.statuses, ...results };
  } catch {
    // Leave these oids unverified — a future call (e.g. the next page fetch, or scrolling
    // back) will retry since they were never added to `state.statuses`.
  } finally {
    for (const oid of toFetch) pendingOids.delete(oid);
  }
}
