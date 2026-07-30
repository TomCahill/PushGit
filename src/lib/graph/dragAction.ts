// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Pure decision logic for the commit graph's drag-to-merge/rebase/reset/move-tag gesture
// (`CommitGraph.svelte`). Kept separate from the component so every combination can be tested
// as plain data, without needing a real pointer-drag simulation for each one.
//
// Two independent rules, picked by the source's kind:
// - A branch source: every valid drag has the current (HEAD) branch as exactly one end, and
//   which end it's on deterministically picks the verb — so there's no drop-time picker to
//   disambiguate. Dragging a non-current branch onto the current one merges it in; dragging the
//   current branch onto something else rebases it onto that target. Anything else (both ends
//   non-current, a tag on either end, a self-drop) is not a valid drag.
//   The one exception is the current branch dropped onto a bare commit row while a modifier key
//   is held (the drag-to-reset item): that's a `reset`, not a `rebase` —
//   the modifier is what disambiguates the two, since the drop shape is otherwise identical.
//   `CommitGraph.svelte` still needs its own soft/mixed/hard mode picker after the drop; this
//   function only decides which verb applies.
// - A tag source: dropped onto any commit or non-tag ref, it moves there (a force-recreate of
//   the tag at the new target — see `branch::move_tag`). Dropped onto another tag, or onto the
//   commit it's already on, it's a no-op.
import type { RefMarker } from "$lib/git/types";

export type DragTarget = { type: "ref"; ref: RefMarker } | { type: "commit"; oid: string };

export type DragActionResult =
  | { verb: "merge"; sourceName: string }
  | { verb: "rebase"; onto: string }
  | { verb: "reset"; onto: string }
  | { verb: "move_tag"; tagName: string; onto: string }
  | null;

export function decideDragAction(
  source: { ref: RefMarker; commitOid: string },
  target: DragTarget,
  options: { modifierHeld?: boolean } = {},
): DragActionResult {
  if (source.ref.kind === "tag") {
    if (target.type === "commit") {
      if (target.oid === source.commitOid) return null;
      return { verb: "move_tag", tagName: source.ref.name, onto: target.oid };
    }
    if (target.ref.kind === "tag") return null;
    return { verb: "move_tag", tagName: source.ref.name, onto: target.ref.name };
  }

  if (target.type === "ref") {
    if (target.ref.kind === "tag") return null;
    if (target.ref.name === source.ref.name && target.ref.kind === source.ref.kind) return null;

    if (!source.ref.isHead && target.ref.isHead) {
      return { verb: "merge", sourceName: source.ref.name };
    }
    if (source.ref.isHead && !target.ref.isHead) {
      return { verb: "rebase", onto: target.ref.name };
    }
    return null;
  }

  // target.type === "commit"
  if (target.oid === source.commitOid) return null;
  if (source.ref.isHead) {
    return options.modifierHeld
      ? { verb: "reset", onto: target.oid }
      : { verb: "rebase", onto: target.oid };
  }
  return null;
}
