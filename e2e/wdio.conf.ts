// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// E2E harness: WebdriverIO + @wdio/tauri-service, `embedded` driver
// provider (the default — runs a WebDriver HTTP server inside the app itself via
// tauri-plugin-wdio-webdriver, no external tauri-driver/webkit2gtk-driver needed). Points at
// a debug binary built with `--features e2e` (see `npm run test:e2e:build`) — never the
// normal dev/release binary, which never links the wdio plugins at all.
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { Options } from "@wdio/types";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const appBinaryPath = path.join(repoRoot, "src-tauri/target/debug/pushgit");

export const config: Options.Testrunner = {
  runner: "local",
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,

  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        captureBackendLogs: true,
        captureFrontendLogs: true,
      },
    ],
  ],

  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinaryPath,
      },
    },
  ],

  // Bump to 'debug' via `wdio run e2e/wdio.conf.ts --logLevel debug` when diagnosing a
  // failure — 'info' dumps every raw WebDriver request/response, which is unusable noise
  // for day-to-day runs.
  logLevel: "warn",
  bail: 0,
  waitforTimeout: 10_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 3,

  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 60_000,
  },

  reporters: ["spec"],
};
