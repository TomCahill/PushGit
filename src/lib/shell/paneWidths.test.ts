// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import {
  parsePaneWidthsJson,
  REPO_RAIL_DEFAULT,
  REPO_RAIL_MAX,
  REPO_RAIL_MIN,
  SIDEBAR_DEFAULT,
  SIDEBAR_MAX,
  SIDEBAR_MIN,
} from "./paneWidths.svelte";

describe("parsePaneWidthsJson", () => {
  it("falls back to the defaults when there's nothing stored", () => {
    expect(parsePaneWidthsJson(null)).toEqual({
      repoRail: REPO_RAIL_DEFAULT,
      sidebar: SIDEBAR_DEFAULT,
    });
  });

  it("returns validly-stored values unchanged", () => {
    expect(parsePaneWidthsJson('{"repoRail":260,"sidebar":600}')).toEqual({
      repoRail: 260,
      sidebar: 600,
    });
  });

  it("falls back to the defaults for malformed JSON", () => {
    expect(parsePaneWidthsJson("not json")).toEqual({
      repoRail: REPO_RAIL_DEFAULT,
      sidebar: SIDEBAR_DEFAULT,
    });
  });

  it("falls back to the default for a repo rail width outside its min/max", () => {
    expect(parsePaneWidthsJson(`{"repoRail":${REPO_RAIL_MIN - 1},"sidebar":600}`)).toEqual({
      repoRail: REPO_RAIL_DEFAULT,
      sidebar: 600,
    });
    expect(parsePaneWidthsJson(`{"repoRail":${REPO_RAIL_MAX + 1},"sidebar":600}`)).toEqual({
      repoRail: REPO_RAIL_DEFAULT,
      sidebar: 600,
    });
  });

  it("falls back to the default for a sidebar width outside its min/max", () => {
    expect(parsePaneWidthsJson(`{"repoRail":260,"sidebar":${SIDEBAR_MIN - 1}}`)).toEqual({
      repoRail: 260,
      sidebar: SIDEBAR_DEFAULT,
    });
    expect(parsePaneWidthsJson(`{"repoRail":260,"sidebar":${SIDEBAR_MAX + 1}}`)).toEqual({
      repoRail: 260,
      sidebar: SIDEBAR_DEFAULT,
    });
  });

  it("falls back to the defaults for non-numeric values", () => {
    expect(parsePaneWidthsJson('{"repoRail":"wide","sidebar":600}')).toEqual({
      repoRail: REPO_RAIL_DEFAULT,
      sidebar: 600,
    });
  });
});
