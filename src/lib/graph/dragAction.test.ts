// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { decideDragAction, type DragTarget } from "./dragAction";
import type { RefMarker } from "$lib/git/types";

function ref(overrides: Partial<RefMarker> = {}): RefMarker {
  return { name: "branch", kind: "local_branch", isHead: false, ...overrides };
}

const HEAD_COMMIT_OID = "head-oid";

describe("decideDragAction", () => {
  it("merges a non-current local branch dropped onto the current branch", () => {
    const source = { ref: ref({ name: "feature", isHead: false }), commitOid: "feature-oid" };
    const target: DragTarget = { type: "ref", ref: ref({ name: "main", isHead: true }) };

    expect(decideDragAction(source, target)).toEqual({ verb: "merge", sourceName: "feature" });
  });

  it("merges a non-current remote branch dropped onto the current branch", () => {
    const source = {
      ref: ref({ name: "origin/feature", kind: "remote_branch", isHead: false }),
      commitOid: "feature-oid",
    };
    const target: DragTarget = { type: "ref", ref: ref({ name: "main", isHead: true }) };

    expect(decideDragAction(source, target)).toEqual({
      verb: "merge",
      sourceName: "origin/feature",
    });
  });

  it("rebases the current branch dropped onto a non-current local branch", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "ref", ref: ref({ name: "feature", isHead: false }) };

    expect(decideDragAction(source, target)).toEqual({ verb: "rebase", onto: "feature" });
  });

  it("rebases the current branch dropped onto a non-current remote branch", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = {
      type: "ref",
      ref: ref({ name: "origin/feature", kind: "remote_branch", isHead: false }),
    };

    expect(decideDragAction(source, target)).toEqual({ verb: "rebase", onto: "origin/feature" });
  });

  it("rebases the current branch dropped onto a bare commit", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "commit", oid: "some-other-oid" };

    expect(decideDragAction(source, target)).toEqual({ verb: "rebase", onto: "some-other-oid" });
  });

  it("resets the current branch dropped onto a bare commit while a modifier key is held", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "commit", oid: "some-other-oid" };

    expect(decideDragAction(source, target, { modifierHeld: true })).toEqual({
      verb: "reset",
      onto: "some-other-oid",
    });
  });

  it("rejects a modifier-held reset drag onto a branch badge (reset only targets bare commits)", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "ref", ref: ref({ name: "feature", isHead: false }) };

    expect(decideDragAction(source, target, { modifierHeld: true })).toEqual({
      verb: "rebase",
      onto: "feature",
    });
  });

  it("rejects a modifier-held reset drag onto the commit the branch is already on", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "commit", oid: HEAD_COMMIT_OID };

    expect(decideDragAction(source, target, { modifierHeld: true })).toBeNull();
  });

  it("moves a tag dropped onto a bare commit", () => {
    const source = { ref: ref({ name: "v1.0.0", kind: "tag" }), commitOid: "tag-oid" };
    const target: DragTarget = { type: "commit", oid: "some-other-oid" };

    expect(decideDragAction(source, target)).toEqual({
      verb: "move_tag",
      tagName: "v1.0.0",
      onto: "some-other-oid",
    });
  });

  it("moves a tag dropped onto a branch badge", () => {
    const source = { ref: ref({ name: "v1.0.0", kind: "tag" }), commitOid: "tag-oid" };
    const target: DragTarget = { type: "ref", ref: ref({ name: "main", isHead: true }) };

    expect(decideDragAction(source, target)).toEqual({
      verb: "move_tag",
      tagName: "v1.0.0",
      onto: "main",
    });
  });

  it("moves a tag dropped onto a remote branch badge", () => {
    const source = { ref: ref({ name: "v1.0.0", kind: "tag" }), commitOid: "tag-oid" };
    const target: DragTarget = {
      type: "ref",
      ref: ref({ name: "origin/main", kind: "remote_branch" }),
    };

    expect(decideDragAction(source, target)).toEqual({
      verb: "move_tag",
      tagName: "v1.0.0",
      onto: "origin/main",
    });
  });

  it("rejects a tag dropped onto another tag", () => {
    const source = { ref: ref({ name: "v1.0.0", kind: "tag" }), commitOid: "tag-oid" };
    const target: DragTarget = { type: "ref", ref: ref({ name: "v2.0.0", kind: "tag" }) };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects dropping a tag onto the commit it's already on", () => {
    const source = { ref: ref({ name: "v1.0.0", kind: "tag" }), commitOid: "tag-oid" };
    const target: DragTarget = { type: "commit", oid: "tag-oid" };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects a tag as the drop target", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "ref", ref: ref({ name: "v1.0.0", kind: "tag" }) };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects a drag between two non-current branches", () => {
    const source = { ref: ref({ name: "feature-a", isHead: false }), commitOid: "a-oid" };
    const target: DragTarget = { type: "ref", ref: ref({ name: "feature-b", isHead: false }) };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects dropping a branch onto itself", () => {
    const source = { ref: ref({ name: "feature", isHead: false }), commitOid: "feature-oid" };
    const target: DragTarget = { type: "ref", ref: ref({ name: "feature", isHead: false }) };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects dropping the current branch onto the commit it's already on", () => {
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = { type: "commit", oid: HEAD_COMMIT_OID };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects a non-current branch dropped onto a bare commit", () => {
    const source = { ref: ref({ name: "feature", isHead: false }), commitOid: "feature-oid" };
    const target: DragTarget = { type: "commit", oid: "some-other-oid" };

    expect(decideDragAction(source, target)).toBeNull();
  });

  it("rejects two branches that both report isHead (backend oid-comparison bug guard)", () => {
    // `graph/session.rs` currently computes `isHead` by comparing target commit oid to HEAD's
    // oid, not branch identity — so two local branches at the same commit can both report
    // `isHead: true`. Neither of this decision function's two isHead-gated rules can fire when
    // both sides claim it, so this must resolve to null rather than picking one arbitrarily.
    const source = { ref: ref({ name: "main", isHead: true }), commitOid: HEAD_COMMIT_OID };
    const target: DragTarget = {
      type: "ref",
      ref: ref({ name: "also-main", isHead: true }),
    };

    expect(decideDragAction(source, target)).toBeNull();
  });
});
