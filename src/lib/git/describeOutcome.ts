// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Human-readable toast text for a merge/rebase/cherry-pick outcome, shared by every panel
// that can trigger one directly (BranchSidebar, RemotePanel, CommitGraph's drag/context-menu
// actions) rather than each reimplementing its own copy of the same switch.

import type { CherryPickOutcome, MergeOutcome, RebaseOutcome } from "./types";

export function describeMergeOutcome(branchName: string, outcome: MergeOutcome): string {
  switch (outcome.kind) {
    case "fast_forward":
      return `Fast-forwarded to ${branchName}.`;
    case "already_up_to_date":
      return "Already up to date.";
    case "merged":
      return `Merged ${branchName}.`;
    case "conflicts":
      return `Merge stopped with ${outcome.conflicts.length} conflicting file(s).`;
  }
}

/** Same outcome type as `describeMergeOutcome`, but pull-flavored wording — kept as its own
 *  function rather than a shared branch-name-optional variant, since "fast-forwarded to X"
 *  doesn't read naturally for a pull against a tracked upstream. */
export function describePullOutcome(outcome: MergeOutcome): string {
  switch (outcome.kind) {
    case "fast_forward":
      return "Pulled — fast-forwarded.";
    case "already_up_to_date":
      return "Already up to date.";
    case "merged":
      return "Pulled and merged.";
    case "conflicts":
      return `Pull stopped with ${outcome.conflicts.length} conflicting file(s).`;
  }
}

export function describeRebaseOutcome(outcome: RebaseOutcome): string {
  switch (outcome.kind) {
    case "completed":
      return "Rebase completed.";
    case "conflicts":
      return `Rebase paused with ${outcome.conflicts.length} conflicting file(s).`;
  }
}

export function describeCherryPickOutcome(outcome: CherryPickOutcome): string {
  switch (outcome.kind) {
    case "cherry_picked":
      return "Cherry-picked onto the current branch.";
    case "conflicts":
      return `Cherry-pick stopped with ${outcome.conflicts.length} conflicting file(s).`;
  }
}
