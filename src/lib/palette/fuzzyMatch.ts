// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

export interface PathMatch {
  path: string;
  name: string;
  dir: string;
  score: number;
  nameHits: number[];
  dirHits: number[];
}

export interface Segment {
  text: string;
  hit: boolean;
}

function subsequence(needle: string, haystack: string): number[] | null {
  const hits: number[] = [];
  let from = 0;
  for (const char of needle) {
    const at = haystack.indexOf(char, from);
    if (at === -1) return null;
    hits.push(at);
    from = at + 1;
  }
  return hits;
}

function quality(hits: number[], text: string): number {
  let score = 0;
  for (let i = 0; i < hits.length; i++) {
    if (i > 0 && hits[i] === hits[i - 1] + 1) score += 5;
    const before = text[hits[i] - 1];
    if (hits[i] === 0 || before === "/" || before === "-" || before === "_" || before === ".") {
      score += 3;
    }
  }
  return score;
}

/** Matches `query` against a path, preferring hits in the file name over the folder path. */
export function fuzzyMatch(query: string, path: string): PathMatch | null {
  const needle = query.toLowerCase().replace(/\s+/g, "");
  const slash = path.lastIndexOf("/");
  const dir = path.slice(0, slash + 1);
  const name = path.slice(slash + 1);
  if (!needle) return { path, name, dir, score: 0, nameHits: [], dirHits: [] };
  const lowerName = name.toLowerCase();
  const lowerDir = dir.toLowerCase();

  // The longest tail of the query that fits in the name wins; the rest must fit in the folders.
  for (let inName = needle.length; inName >= 0; inName--) {
    const nameHits = subsequence(needle.slice(needle.length - inName), lowerName);
    if (!nameHits) continue;
    const dirHits = subsequence(needle.slice(0, needle.length - inName), lowerDir);
    if (!dirHits) continue;
    const score =
      inName * 20 +
      (inName === needle.length ? 200 : 0) +
      quality(nameHits, lowerName) +
      quality(dirHits, lowerDir) -
      path.length * 0.01;
    return { path, name, dir, score, nameHits, dirHits };
  }
  return null;
}

/** The best `limit` matches, best first, without sorting every match. */
export function topMatches(query: string, paths: readonly string[], limit: number): PathMatch[] {
  const best: PathMatch[] = [];
  for (const path of paths) {
    const match = fuzzyMatch(query, path);
    if (!match) continue;
    if (best.length === limit && match.score <= best[best.length - 1].score) continue;
    let at = best.length;
    while (at > 0 && best[at - 1].score < match.score) at--;
    best.splice(at, 0, match);
    if (best.length > limit) best.pop();
  }
  return best;
}

export function segments(text: string, hits: number[]): Segment[] {
  const hitSet = new Set(hits);
  const out: Segment[] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = hitSet.has(i);
    const last = out[out.length - 1];
    if (last && last.hit === hit) last.text += text[i];
    else out.push({ text: text[i], hit });
  }
  return out;
}
