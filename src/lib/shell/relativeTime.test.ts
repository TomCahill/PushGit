// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { formatRelativeTime } from "./relativeTime";

const NOW = Date.UTC(2026, 10, 2, 12, 0, 0);
const MINUTE = 60_000;

describe("formatRelativeTime", () => {
  it.each([
    [NOW - 20_000, "just now"],
    [NOW - 5 * MINUTE, "5 minutes ago"],
    [NOW - 2 * 60 * MINUTE, "2 hours ago"],
    [NOW - 24 * 60 * MINUTE, "yesterday"],
    [NOW - 3 * 24 * 60 * MINUTE, "3 days ago"],
  ])("formats %i relative to now", (timestamp, expected) => {
    expect(formatRelativeTime(timestamp, NOW)).toBe(expected);
  });
});
