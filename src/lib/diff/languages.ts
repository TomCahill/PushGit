// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Maps a file extension to one of `highlight.js`'s language ids statically imported by
// `./highlight.ts` (see that file's header comment for why they're static, not lazy).
// Extensions with no confident mapping fall back to no highlighting (plain rendering)
// rather than guessing, matching `detectLanguage`'s null-safe contract.
const EXTENSION_TO_LANGUAGE: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  py: "python",
  rb: "ruby",
  go: "go",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  cs: "csharp",
  php: "php",
  swift: "swift",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  json: "json",
  yaml: "yaml",
  yml: "yaml",
  html: "xml",
  htm: "xml",
  xml: "xml",
  svg: "xml",
  css: "css",
  scss: "scss",
  less: "less",
  md: "markdown",
  markdown: "markdown",
  sql: "sql",
  graphql: "graphql",
  dockerfile: "dockerfile",
  ini: "ini",
};

/** The `highlight.js` language id for `path`'s extension, or `null` if there's no mapping. */
export function detectLanguage(path: string | null): string | null {
  if (!path) return null;
  const match = /\.([^./]+)$/.exec(path);
  const ext = match?.[1]?.toLowerCase();
  if (!ext) return null;
  return EXTENSION_TO_LANGUAGE[ext] ?? null;
}
