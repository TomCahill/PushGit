// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import { motionBase, motionFast, prefersReducedMotion } from "./motion";

describe("motion", () => {
  it("falls back to the documented defaults when the CSS custom property isn't readable", () => {
    // jsdom doesn't compute custom properties from component <style> tags, so this also
    // pins down the fallback values themselves as a regression guard.
    expect(motionFast()).toBe(120);
    expect(motionBase()).toBe(200);
  });

  describe("prefersReducedMotion", () => {
    afterEach(() => {
      document.documentElement.removeAttribute("data-reduce-motion");
    });

    it("is false when neither the setting attribute nor the media query is set", () => {
      expect(prefersReducedMotion()).toBe(false);
    });

    it("is true when the data-reduce-motion attribute is set (app setting)", () => {
      document.documentElement.setAttribute("data-reduce-motion", "true");
      expect(prefersReducedMotion()).toBe(true);
    });

    it("is true when prefers-reduced-motion: reduce matches (OS setting)", () => {
      const originalMatchMedia = window.matchMedia;
      window.matchMedia = (query: string) =>
        ({ matches: query === "(prefers-reduced-motion: reduce)" }) as MediaQueryList;

      expect(prefersReducedMotion()).toBe(true);

      window.matchMedia = originalMatchMedia;
    });
  });
});
