// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fuzzyMatch, segments, topMatches } from "./fuzzyMatch";

describe("fuzzyMatch", () => {
  it("splits a query across folder and file name", () => {
    const match = fuzzyMatch("blmod", "src-tauri/src/blame/mod.rs");
    expect(match).not.toBeNull();
    expect(match!.nameHits).toEqual([0, 1, 2]);
    expect(match!.dirHits.map((i) => match!.dir[i]).join("")).toBe("bl");
  });

  it("rejects paths that don't contain the letters in order", () => {
    expect(fuzzyMatch("zq", "src/lib.rs")).toBeNull();
    expect(fuzzyMatch("rsb", "src/lib.rs")).toBeNull();
  });

  it("is case-insensitive and ignores spaces", () => {
    expect(fuzzyMatch("BLAME view", "src/lib/blame/BlameView.svelte")).not.toBeNull();
  });

  it("ranks a whole-name match above a match spread over the path", () => {
    const ranked = topMatches("mod", ["src/model/thing.rs", "src/blame/mod.rs"], 10);
    expect(ranked.map((m) => m.path)).toEqual(["src/blame/mod.rs", "src/model/thing.rs"]);
  });

  it("prefers contiguous and word-start hits", () => {
    const ranked = topMatches("view", ["src/very_iffy_ew.ts", "src/BlameView.svelte"], 10);
    expect(ranked[0].path).toBe("src/BlameView.svelte");
  });

  it("keeps only the best `limit` matches, best first", () => {
    const paths = ["a/x.ts", "b/xx.ts", "c/xxx.ts", "d/x.rs"];
    const ranked = topMatches("x", paths, 2);
    expect(ranked).toHaveLength(2);
    expect(ranked[0].score).toBeGreaterThanOrEqual(ranked[1].score);
  });
});

describe("segments", () => {
  it("groups adjacent hit and miss characters", () => {
    expect(segments("mod.rs", [0, 1, 2])).toEqual([
      { text: "mod", hit: true },
      { text: ".rs", hit: false },
    ]);
  });
});
