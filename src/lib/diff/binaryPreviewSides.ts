// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { FileStatus } from "$lib/git/types";

// `FileDiff.oldPath`/`newPath` aren't a reliable signal for "does this side actually have
// content" — git2 mirrors `newPath` into `oldPath` (and vice versa) even for a pure add/delete
// delta, where the mirrored path doesn't exist in that side's tree/index/blob at all. `status`
// is the only reliable signal, so the binary-preview resolvers key off it instead.

export function hasOldSide(status: FileStatus): boolean {
  return status !== "added" && status !== "untracked";
}

export function hasNewSide(status: FileStatus): boolean {
  return status !== "deleted";
}
