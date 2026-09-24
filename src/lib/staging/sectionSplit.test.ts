// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  parseUnstagedFraction,
  resizedUnstagedFraction,
  SECTION_MIN_PX,
  UNSTAGED_FRACTION_DEFAULT,
} from "./sectionSplit.svelte";

describe("parseUnstagedFraction", () => {
  it("falls back to the default when there's nothing stored", () => {
    expect(parseUnstagedFraction(null, null)).toBe(UNSTAGED_FRACTION_DEFAULT);
  });

  it("returns a validly-stored fraction unchanged", () => {
    expect(parseUnstagedFraction('{"unstagedFraction":0.3}', null)).toBe(0.3);
  });

  it("falls back to the default for malformed JSON", () => {
    expect(parseUnstagedFraction("not json", null)).toBe(UNSTAGED_FRACTION_DEFAULT);
  });

  it("falls back to the default for a fraction outside (0, 1) or non-numeric", () => {
    expect(parseUnstagedFraction('{"unstagedFraction":0}', null)).toBe(UNSTAGED_FRACTION_DEFAULT);
    expect(parseUnstagedFraction('{"unstagedFraction":1}', null)).toBe(UNSTAGED_FRACTION_DEFAULT);
    expect(parseUnstagedFraction('{"unstagedFraction":"half"}', null)).toBe(
      UNSTAGED_FRACTION_DEFAULT,
    );
  });

  it("migrates legacy pixel heights to a fraction", () => {
    expect(parseUnstagedFraction(null, '{"unstaged":300,"staged":100}')).toBe(0.75);
  });

  it("prefers the current format over legacy heights", () => {
    expect(parseUnstagedFraction('{"unstagedFraction":0.2}', '{"unstaged":300,"staged":100}')).toBe(
      0.2,
    );
  });

  it("falls back to the default for invalid legacy heights", () => {
    expect(parseUnstagedFraction(null, '{"unstaged":0,"staged":100}')).toBe(
      UNSTAGED_FRACTION_DEFAULT,
    );
    expect(parseUnstagedFraction(null, "not json")).toBe(UNSTAGED_FRACTION_DEFAULT);
  });
});

describe("resizedUnstagedFraction", () => {
  it("grows the unstaged section when dragged down and shrinks it when dragged up", () => {
    expect(resizedUnstagedFraction(0.5, 100, 1000)).toBeCloseTo(0.6);
    expect(resizedUnstagedFraction(0.5, -100, 1000)).toBeCloseTo(0.4);
  });

  it("keeps both sections at least the minimum height", () => {
    expect(resizedUnstagedFraction(0.5, -1000, 1000)).toBeCloseTo(SECTION_MIN_PX / 1000);
    expect(resizedUnstagedFraction(0.5, 1000, 1000)).toBeCloseTo(1 - SECTION_MIN_PX / 1000);
  });

  it("leaves the fraction alone when there's no room for both minimums", () => {
    expect(resizedUnstagedFraction(0.4, 50, 2 * SECTION_MIN_PX)).toBe(0.4);
    expect(resizedUnstagedFraction(0.4, 50, 0)).toBe(0.4);
  });
});
