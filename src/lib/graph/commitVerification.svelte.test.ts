// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { mockIPC } from "@tauri-apps/api/mocks";
import {
  requestVerification,
  resetCommitVerification,
  verificationStatus,
} from "./commitVerification.svelte";

describe("commitVerification", () => {
  it("fetches once and caches the result by oid — a repeat call for the same oid doesn't re-fetch", async () => {
    let calls = 0;
    mockIPC((cmd, args) => {
      if (cmd !== "verify_commits") throw new Error(`unexpected command ${cmd}`);
      calls++;
      const { oids } = args as { oids: string[] };
      return Object.fromEntries(oids.map((oid) => [oid, { status: "good", signer: "Ada" }]));
    });
    resetCommitVerification("/repo-verify-cache");

    await requestVerification("/repo-verify-cache", ["a"]);
    expect(verificationStatus("a")).toEqual({ status: "good", signer: "Ada" });
    expect(calls).toBe(1);

    await requestVerification("/repo-verify-cache", ["a"]);
    expect(calls).toBe(1);
  });

  it("only requests oids not already cached from an earlier call", async () => {
    const requested: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd !== "verify_commits") throw new Error(`unexpected command ${cmd}`);
      const { oids } = args as { oids: string[] };
      requested.push(...oids);
      return Object.fromEntries(oids.map((oid) => [oid, { status: "unsigned" }]));
    });
    resetCommitVerification("/repo-verify-partial");

    await requestVerification("/repo-verify-partial", ["a", "b"]);
    await requestVerification("/repo-verify-partial", ["a", "b", "c"]);

    expect(requested).toEqual(["a", "b", "c"]);
  });

  it("clears the cache on a real repo switch, but leaves it alone for a same-repo call", async () => {
    mockIPC((cmd, args) => {
      if (cmd !== "verify_commits") throw new Error(`unexpected command ${cmd}`);
      const { oids } = args as { oids: string[] };
      return Object.fromEntries(oids.map((oid) => [oid, { status: "bad" }]));
    });

    resetCommitVerification("/repo-a");
    await requestVerification("/repo-a", ["x"]);
    expect(verificationStatus("x")).toEqual({ status: "bad" });

    resetCommitVerification("/repo-a");
    expect(verificationStatus("x")).toEqual({ status: "bad" });

    resetCommitVerification("/repo-b");
    expect(verificationStatus("x")).toBeUndefined();
  });

  it("leaves an oid unverified (not permanently cached as failed) when the backend call rejects", async () => {
    mockIPC((cmd) => {
      if (cmd !== "verify_commits") throw new Error(`unexpected command ${cmd}`);
      throw new Error("git error: not a repository");
    });
    resetCommitVerification("/repo-verify-error");

    await requestVerification("/repo-verify-error", ["a"]);

    expect(verificationStatus("a")).toBeUndefined();
  });
});
