// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import type { Line } from "$lib/git/types";
import { lineCheckboxGroups } from "./lineCheckboxGroups";

function line(overrides: Partial<Line>): Line {
  return { origin: "context", content: "", oldLineno: null, newLineno: null, ...overrides };
}

describe("lineCheckboxGroups", () => {
  it("gives a context line no checkbox", () => {
    const context = line({ origin: "context", content: "same\n" });
    expect(lineCheckboxGroups([context])).toEqual([null]);
  });

  it("gives a standalone pure addition its own single-line group", () => {
    const addition = line({ origin: "addition", content: "new\n" });
    expect(lineCheckboxGroups([addition])).toEqual([[0]]);
  });

  it("gives a standalone pure deletion its own single-line group", () => {
    const deletion = line({ origin: "deletion", content: "old\n" });
    expect(lineCheckboxGroups([deletion])).toEqual([[0]]);
  });

  it("keeps a pure-addition run independently selectable, one group per line", () => {
    const a = line({ origin: "addition", content: "a\n" });
    const b = line({ origin: "addition", content: "b\n" });
    expect(lineCheckboxGroups([a, b])).toEqual([[0], [1]]);
  });

  it("keeps a pure-deletion run independently selectable, one group per line", () => {
    const a = line({ origin: "deletion", content: "a\n" });
    const b = line({ origin: "deletion", content: "b\n" });
    expect(lineCheckboxGroups([a, b])).toEqual([[0], [1]]);
  });

  it("collapses a one-deletion-one-addition edited line into a single group on the first line", () => {
    const deletion = line({ origin: "deletion", content: "old\n" });
    const addition = line({ origin: "addition", content: "new\n" });
    expect(lineCheckboxGroups([deletion, addition])).toEqual([[0, 1], null]);
  });

  it("collapses a one-deletion-many-additions edit into a single group", () => {
    const deletion = line({ origin: "deletion", content: "old\n" });
    const add1 = line({ origin: "addition", content: "new1\n" });
    const add2 = line({ origin: "addition", content: "new2\n" });
    expect(lineCheckboxGroups([deletion, add1, add2])).toEqual([[0, 1, 2], null, null]);
  });

  it("collapses a many-deletions-one-addition edit into a single group", () => {
    const del1 = line({ origin: "deletion", content: "old1\n" });
    const del2 = line({ origin: "deletion", content: "old2\n" });
    const addition = line({ origin: "addition", content: "new\n" });
    expect(lineCheckboxGroups([del1, del2, addition])).toEqual([[0, 1, 2], null, null]);
  });

  it("treats context as a hard boundary between two separate groups", () => {
    const del1 = line({ origin: "deletion", content: "old1\n" });
    const add1 = line({ origin: "addition", content: "new1\n" });
    const context = line({ origin: "context", content: "keep\n" });
    const del2 = line({ origin: "deletion", content: "old2\n" });
    const add2 = line({ origin: "addition", content: "new2\n" });

    expect(lineCheckboxGroups([del1, add1, context, del2, add2])).toEqual([
      [0, 1],
      null,
      null,
      [3, 4],
      null,
    ]);
  });

  it("returns an empty array for an empty hunk", () => {
    expect(lineCheckboxGroups([])).toEqual([]);
  });
});
