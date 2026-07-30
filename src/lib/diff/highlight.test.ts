// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { highlightSource, splitHighlightedHtml } from "./highlight";

describe("highlightSource", () => {
  it("tokenizes recognized syntax into highlight.js's own escaped spans", () => {
    const html = highlightSource("const x = 1;\n", "javascript");

    expect(html).toContain('<span class="hljs-keyword">const</span>');
    expect(html).toContain('<span class="hljs-number">1</span>');
  });

  it("escapes source text that looks like markup", () => {
    const html = highlightSource('const s = "<b>";\n', "javascript");

    expect(html).not.toContain("<b>");
    expect(html).toContain("&lt;b&gt;");
  });

  it("returns null for an unregistered/unknown language id", () => {
    expect(highlightSource("const x = 1;", "not-a-real-language")).toBeNull();
  });
});

describe("splitHighlightedHtml", () => {
  it("splits plain single-line-per-entry HTML back into matching fragments", () => {
    const html = '<span class="hljs-keyword">const</span> x = <span class="hljs-number">1</span>;';

    expect(splitHighlightedHtml(html, 1)).toEqual([html]);
  });

  it("re-opens a span that crosses a line boundary so each fragment is self-contained", () => {
    // A block comment spanning three lines, followed by a fourth unrelated line — exactly
    // what highlight.js produces for `/* start\nmiddle\nend */\ncode`.
    const html =
      '<span class="hljs-comment">/* start\nmiddle\nend */</span>\n<span class="hljs-keyword">const</span>;';

    const lines = splitHighlightedHtml(html, 4);

    expect(lines).toEqual([
      '<span class="hljs-comment">/* start</span>',
      '<span class="hljs-comment">middle</span>',
      '<span class="hljs-comment">end */</span>',
      '<span class="hljs-keyword">const</span>;',
    ]);
  });

  it("re-opens multiple nested spans in the same order they were opened", () => {
    const html = '<span class="a"><span class="b">x\ny</span></span>';

    const lines = splitHighlightedHtml(html, 2);

    expect(lines).toEqual([
      '<span class="a"><span class="b">x</span></span>',
      '<span class="a"><span class="b">y</span></span>',
    ]);
  });

  it("drops the trailing empty segment produced by a fully newline-terminated source", () => {
    const html = '<span class="hljs-keyword">const</span>;\n';

    expect(splitHighlightedHtml(html, 1)).toEqual(['<span class="hljs-keyword">const</span>;']);
  });

  it("round-trips real highlight.js output for a multi-line block comment", () => {
    const source = "/* start\nmiddle\nend */\nconst x = 1;\n";
    const html = highlightSource(source, "javascript");

    const lines = splitHighlightedHtml(html!, 4);

    expect(lines).toHaveLength(4);
    expect(lines[0]).toContain("/* start");
    expect(lines[1]).toContain("middle");
    expect(lines[2]).toContain("end */");
    expect(lines[3]).toContain('<span class="hljs-keyword">const</span>');
    // Every fragment must independently balance — an unclosed <span> would break isolated
    // rendering into one row per line.
    for (const line of lines) {
      const opens = (line.match(/<span /g) ?? []).length;
      const closes = (line.match(/<\/span>/g) ?? []).length;
      expect(opens).toBe(closes);
    }
  });
});
