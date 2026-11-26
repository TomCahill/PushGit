// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import type { FileStatus, TreeSource } from "$lib/git/types";

export const WORKING_TREE: TreeSource = { kind: "workingTree" };

export const exploreState: {
  source: TreeSource;
  /** The branch/tag/rev the user picked, or null when browsing a bare commit or the working tree. */
  refLabel: string | null;
  path: string | null;
  status: FileStatus | null;
  /** A path the tree should expand to and scroll into view. */
  reveal: string | null;
} = $state({ source: WORKING_TREE, refLabel: null, path: null, status: null, reveal: null });

export function browse(source: TreeSource, refLabel: string | null, path: string | null = null) {
  exploreState.source = source;
  exploreState.refLabel = refLabel;
  exploreState.path = path;
  exploreState.status = null;
  exploreState.reveal = path;
}

export function openFile(path: string, status: FileStatus | null) {
  exploreState.path = path;
  exploreState.status = status;
}

export function resetExplore() {
  browse(WORKING_TREE, null);
}

export function sourceKey(source: TreeSource): string {
  return source.kind === "revision" ? source.oid : "workdir";
}

export function shortOid(oid: string): string {
  return oid.slice(0, 7);
}
