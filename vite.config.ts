/// <reference types="vitest/config" />
// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const host = process.env.TAURI_DEV_HOST;
const isVitest = process.env.VITEST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  // Component/unit tests via Vitest + @testing-library/svelte
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    include: ["src/**/*.{test,spec}.{js,ts}", "src/**/*.svelte.{test,spec}.{js,ts}"],
    coverage: {
      provider: "v8" as const,
      include: ["src/**/*.{js,ts,svelte}"],
    },
  },
  // The sveltekit() plugin resolves Svelte to its server-side build by default; Vitest
  // needs the browser condition forced so component tests get the client runtime instead.
  resolve: isVitest ? { conditions: ["browser"] } : undefined,
}));
