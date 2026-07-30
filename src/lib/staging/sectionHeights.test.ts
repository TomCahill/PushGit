// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  parseSectionHeightsJson,
  STAGED_DEFAULT,
  STAGED_MAX,
  STAGED_MIN,
  UNSTAGED_DEFAULT,
  UNSTAGED_MAX,
  UNSTAGED_MIN,
} from "./sectionHeights.svelte";

describe("parseSectionHeightsJson", () => {
  it("falls back to the defaults when there's nothing stored", () => {
    expect(parseSectionHeightsJson(null)).toEqual({
      unstaged: UNSTAGED_DEFAULT,
      staged: STAGED_DEFAULT,
    });
  });

  it("returns validly-stored values unchanged", () => {
    expect(parseSectionHeightsJson('{"unstaged":200,"staged":180}')).toEqual({
      unstaged: 200,
      staged: 180,
    });
  });

  it("falls back to the defaults for malformed JSON", () => {
    expect(parseSectionHeightsJson("not json")).toEqual({
      unstaged: UNSTAGED_DEFAULT,
      staged: STAGED_DEFAULT,
    });
  });

  it("falls back to the default for an unstaged height outside its min/max", () => {
    expect(parseSectionHeightsJson(`{"unstaged":${UNSTAGED_MIN - 1},"staged":180}`)).toEqual({
      unstaged: UNSTAGED_DEFAULT,
      staged: 180,
    });
    expect(parseSectionHeightsJson(`{"unstaged":${UNSTAGED_MAX + 1},"staged":180}`)).toEqual({
      unstaged: UNSTAGED_DEFAULT,
      staged: 180,
    });
  });

  it("falls back to the default for a staged height outside its min/max", () => {
    expect(parseSectionHeightsJson(`{"unstaged":200,"staged":${STAGED_MIN - 1}}`)).toEqual({
      unstaged: 200,
      staged: STAGED_DEFAULT,
    });
    expect(parseSectionHeightsJson(`{"unstaged":200,"staged":${STAGED_MAX + 1}}`)).toEqual({
      unstaged: 200,
      staged: STAGED_DEFAULT,
    });
  });

  it("falls back to the defaults for non-numeric values", () => {
    expect(parseSectionHeightsJson('{"unstaged":"tall","staged":180}')).toEqual({
      unstaged: UNSTAGED_DEFAULT,
      staged: 180,
    });
  });
});
