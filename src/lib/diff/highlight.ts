// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// `highlight.js`'s core (no languages bundled) plus a fixed, statically-imported set of
// language grammars matching `./languages.ts`'s extension map, registered once at module
// load. A per-language *dynamic* `import()` was tried first to shave bundle size further,
// but a bare `node_modules` specifier built from a runtime variable (`` `.../${language}` ``)
// isn't something Vite/Rollup can resolve into a real chunk — it survives into the built
// output as a literal template string, which fails at runtime in the shipped app (browsers
// can't resolve bare specifiers without an import map). Confirmed by inspecting
// `npm run build`'s output. Static imports below are correctness-guaranteed and, per
// `ARCHITECTURE.md` §9's <15MB install-size target, cheap enough in absolute terms
// (~25 language grammars, each a few KB) not to be worth the complexity of a real fix
// (e.g. a build-time `import.meta.glob` map) unless the language list grows much further.
import hljs from "highlight.js/lib/core";
import type { LanguageFn } from "highlight.js";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import graphql from "highlight.js/lib/languages/graphql";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import less from "highlight.js/lib/languages/less";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

// Keep in sync with `./languages.ts`'s `EXTENSION_TO_LANGUAGE` value set.
const LANGUAGES: Record<string, LanguageFn> = {
  bash,
  c,
  cpp,
  csharp,
  css,
  dockerfile,
  go,
  graphql,
  ini,
  java,
  javascript,
  json,
  kotlin,
  less,
  markdown,
  php,
  python,
  ruby,
  rust,
  scss,
  sql,
  swift,
  typescript,
  xml,
  yaml,
};

for (const [name, language] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, language);
}

/**
 * Highlights `source` as `language`, returning `highlight.js`'s own HTML (it escapes the
 * underlying text itself — safe to render via `{@html}`), or `null` if `language` isn't one
 * of the registered ids above, in which case the caller should fall back to plain text.
 */
export function highlightSource(source: string, language: string): string | null {
  if (!hljs.getLanguage(language)) return null;
  return hljs.highlight(source, { language, ignoreIllegals: true }).value;
}

/**
 * Splits `html` — balanced `<span class="...">...</span>` output from `highlightSource` for
 * a whole hunk's worth of joined source lines — back into `lineCount` self-contained
 * per-line fragments, one per original source line. A single token (e.g. a block comment)
 * can span multiple lines in the balanced output; this closes every span still open right
 * before each line break and reopens the same ones at the start of the next line, so each
 * returned fragment is independently well-formed HTML.
 */
export function splitHighlightedHtml(html: string, lineCount: number): string[] {
  const tagPattern = /<span class="([^"]*)">|<\/span>|\n/g;
  const openClasses: string[] = [];
  const lines: string[] = [];
  let current = "";
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = tagPattern.exec(html))) {
    current += html.slice(lastIndex, match.index);
    lastIndex = tagPattern.lastIndex;

    if (match[0] === "\n") {
      current += "</span>".repeat(openClasses.length);
      lines.push(current);
      current = openClasses.map((cls) => `<span class="${cls}">`).join("");
    } else if (match[0] === "</span>") {
      openClasses.pop();
      current += "</span>";
    } else {
      openClasses.push(match[1]);
      current += match[0];
    }
  }
  current += html.slice(lastIndex);
  lines.push(current);

  // A highlighted string ending in "\n" (the common case — every `Line.content` from the
  // backend keeps its own trailing newline) produces one trailing empty segment beyond the
  // original line count; drop it so the result lines up 1:1 with the source lines.
  while (lines.length > lineCount) lines.pop();
  return lines;
}
