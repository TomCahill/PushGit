// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { motionBase, motionFast } from "./motion";

describe("motion", () => {
  it("falls back to the documented defaults when the CSS custom property isn't readable", () => {
    // jsdom doesn't compute custom properties from component <style> tags, so this also
    // pins down the fallback values themselves as a regression guard.
    expect(motionFast()).toBe(120);
    expect(motionBase()).toBe(200);
  });
});
