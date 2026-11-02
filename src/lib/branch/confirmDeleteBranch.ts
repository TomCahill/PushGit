// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { branchUnmergedCount } from "$lib/git/api";
import { confirmAsync } from "$lib/shell/confirmDialog.svelte";

// libgit2 deletes unmerged branches without complaint, so warn here before commits are orphaned.
export async function confirmDeleteBranch(repoPath: string, name: string): Promise<boolean> {
  // A missing warning beats a blocked delete, so a failed count falls back to the plain prompt.
  const unmerged = await branchUnmergedCount(repoPath, name).catch(() => 0);
  if (unmerged === 0) {
    return confirmAsync(`Branch "${name}" will be deleted.`, {
      title: "Delete branch?",
      confirmLabel: "Delete branch",
      danger: true,
    });
  }
  const commits = unmerged === 1 ? "1 commit that isn't" : `${unmerged} commits that aren't`;
  return confirmAsync(
    `"${name}" has ${commits} on any other branch. You can undo this with the toolbar's Undo button.`,
    { title: "Delete unmerged branch?", confirmLabel: "Delete anyway", danger: true },
  );
}
