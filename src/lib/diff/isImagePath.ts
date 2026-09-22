// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Extension → MIME type for the raster image formats `ImageDiff.svelte` renders a real
// before/after comparison for — see `.private/feature/image-binary-diff/PLAN.md`. A separate
// table from `languages.ts`'s extension-to-highlight-language map: different domain (MIME
// type, not a `highlight.js` language id) and a different fallback contract (`detectLanguage`
// returns `null` for "no highlighting"; this needs a real MIME string to build a `data:` URI).
const EXTENSION_TO_MIME_TYPE: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  ico: "image/x-icon",
};

function extensionOf(path: string): string | null {
  const match = /\.([^./]+)$/.exec(path);
  return match?.[1]?.toLowerCase() ?? null;
}

/** True when `path`'s extension is one of the raster image formats this app renders a real
 *  before/after preview for. `null` (no path) is never an image. */
export function isImagePath(path: string | null): boolean {
  if (!path) return false;
  const ext = extensionOf(path);
  return ext !== null && ext in EXTENSION_TO_MIME_TYPE;
}

/** The MIME type for `path`'s extension, for building a `data:` URI — only meaningful when
 *  `isImagePath(path)` is true; falls back to a generic octet-stream type otherwise rather
 *  than throwing, so a caller that skips the `isImagePath` check first still gets a value. */
export function imageMimeType(path: string): string {
  const ext = extensionOf(path);
  return (ext && EXTENSION_TO_MIME_TYPE[ext]) ?? "application/octet-stream";
}
