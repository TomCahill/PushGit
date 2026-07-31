// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { colorForIdentity } from "./avatarColor";

describe("colorForIdentity", () => {
  it("is stable for the same seed", () => {
    expect(colorForIdentity("ada@example.com")).toBe(colorForIdentity("ada@example.com"));
  });

  it("varies across a spread of typical seeds", () => {
    const seeds = [
      "ada@example.com",
      "grace@example.com",
      "linus@example.com",
      "margaret@example.com",
      "alan@example.com",
    ];
    const colors = new Set(seeds.map(colorForIdentity));
    expect(colors.size).toBeGreaterThan(1);
  });

  it("always returns a color from the fixed palette", () => {
    const color = colorForIdentity("someone@example.com");
    expect(color).toMatch(/^#[0-9A-Fa-f]{6}$/);
  });
});
