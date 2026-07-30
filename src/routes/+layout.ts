// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
export const ssr = false;

// WebdriverIO's Tauri plugin — only present when the frontend was
// built with VITE_E2E=true (see the `test:e2e:build` script), so this test-only
// instrumentation never ships in a normal `npm run build`/`tauri build`.
if (import.meta.env.VITE_E2E) {
  void import("@wdio/tauri-plugin");
}
