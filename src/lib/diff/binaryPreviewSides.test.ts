// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { hasNewSide, hasOldSide } from "./binaryPreviewSides";
import type { FileStatus } from "$lib/git/types";

describe("hasOldSide", () => {
  it.each<[FileStatus, boolean]>([
    ["added", false],
    ["untracked", false],
    ["deleted", true],
    ["modified", true],
    ["renamed", true],
    ["copied", true],
    ["typechange", true],
    ["conflicted", true],
    ["unreadable", true],
  ])("%s -> %s", (status, expected) => {
    expect(hasOldSide(status)).toBe(expected);
  });
});

describe("hasNewSide", () => {
  it.each<[FileStatus, boolean]>([
    ["deleted", false],
    ["added", true],
    ["untracked", true],
    ["modified", true],
    ["renamed", true],
    ["copied", true],
    ["typechange", true],
    ["conflicted", true],
    ["unreadable", true],
  ])("%s -> %s", (status, expected) => {
    expect(hasNewSide(status)).toBe(expected);
  });
});
