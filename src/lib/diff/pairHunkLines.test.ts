// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import type { Line } from "$lib/git/types";
import { pairHunkLines } from "./pairHunkLines";

function line(overrides: Partial<Line>): Line {
  return { origin: "context", content: "", oldLineno: null, newLineno: null, ...overrides };
}

describe("pairHunkLines", () => {
  it("renders a context line identically on both sides", () => {
    const context = line({ origin: "context", content: "same\n", oldLineno: 1, newLineno: 1 });

    const rows = pairHunkLines([context]);

    expect(rows).toEqual([
      { left: { line: context, index: 0 }, right: { line: context, index: 0 } },
    ]);
  });

  it("zips a matched deletion/addition change block pairwise", () => {
    const deletion = line({ origin: "deletion", content: "old\n", oldLineno: 1 });
    const addition = line({ origin: "addition", content: "new\n", newLineno: 1 });

    const rows = pairHunkLines([deletion, addition]);

    expect(rows).toEqual([
      { left: { line: deletion, index: 0 }, right: { line: addition, index: 1 } },
    ]);
  });

  it("pads the shorter side of an unbalanced change block with a blank cell", () => {
    const deletion = line({ origin: "deletion", content: "removed\n", oldLineno: 1 });
    const addition1 = line({ origin: "addition", content: "added 1\n", newLineno: 1 });
    const addition2 = line({ origin: "addition", content: "added 2\n", newLineno: 2 });

    const rows = pairHunkLines([deletion, addition1, addition2]);

    expect(rows).toEqual([
      { left: { line: deletion, index: 0 }, right: { line: addition1, index: 1 } },
      { left: null, right: { line: addition2, index: 2 } },
    ]);
  });

  it("puts a pure deletion run (no following additions) only on the left", () => {
    const deletion1 = line({ origin: "deletion", content: "a\n", oldLineno: 1 });
    const deletion2 = line({ origin: "deletion", content: "b\n", oldLineno: 2 });
    const context = line({ origin: "context", content: "c\n", oldLineno: 3, newLineno: 1 });

    const rows = pairHunkLines([deletion1, deletion2, context]);

    expect(rows).toEqual([
      { left: { line: deletion1, index: 0 }, right: null },
      { left: { line: deletion2, index: 1 }, right: null },
      { left: { line: context, index: 2 }, right: { line: context, index: 2 } },
    ]);
  });

  it("puts a pure addition run (no preceding deletions) only on the right", () => {
    const addition = line({ origin: "addition", content: "new\n", newLineno: 1 });

    const rows = pairHunkLines([addition]);

    expect(rows).toEqual([{ left: null, right: { line: addition, index: 0 } }]);
  });

  it("handles multiple change blocks separated by context in one hunk", () => {
    const del1 = line({ origin: "deletion", content: "old1\n", oldLineno: 1 });
    const add1 = line({ origin: "addition", content: "new1\n", newLineno: 1 });
    const context = line({ origin: "context", content: "keep\n", oldLineno: 2, newLineno: 2 });
    const del2 = line({ origin: "deletion", content: "old2\n", oldLineno: 3 });
    const add2 = line({ origin: "addition", content: "new2\n", newLineno: 3 });

    const rows = pairHunkLines([del1, add1, context, del2, add2]);

    expect(rows).toEqual([
      { left: { line: del1, index: 0 }, right: { line: add1, index: 1 } },
      { left: { line: context, index: 2 }, right: { line: context, index: 2 } },
      { left: { line: del2, index: 3 }, right: { line: add2, index: 4 } },
    ]);
  });

  it("returns no rows for an empty hunk", () => {
    expect(pairHunkLines([])).toEqual([]);
  });
});
