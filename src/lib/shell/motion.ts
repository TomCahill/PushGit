// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Reads the `--motion-fast`/`--motion-base` custom properties `+page.svelte`'s `:root` block
// defines, so Svelte's JS-driven `transition:` directives (which need a numeric millisecond
// duration, not a CSS var) share one source of truth with the CSS-only transitions and the
// `data-reduce-motion`/`prefers-reduced-motion` mechanism that already zeroes those properties
// (`.private/feature/reduce-motion-setting/PLAN.md`). Falls back to the same values the CSS
// declares when `document` isn't available or the property can't be read (e.g. a test's jsdom
// environment, which doesn't reliably compute custom properties from component `<style>` tags).
const FALLBACK_MOTION_FAST_MS = 120;
const FALLBACK_MOTION_BASE_MS = 200;

function readMs(propertyName: string, fallback: number): number {
  if (typeof document === "undefined") return fallback;
  const raw = getComputedStyle(document.documentElement).getPropertyValue(propertyName).trim();
  const parsed = parseFloat(raw);
  return Number.isFinite(parsed) ? parsed : fallback;
}

export function motionFast(): number {
  return readMs("--motion-fast", FALLBACK_MOTION_FAST_MS);
}

export function motionBase(): number {
  return readMs("--motion-base", FALLBACK_MOTION_BASE_MS);
}

// The `data-reduce-motion`/`@media` mechanism above only zeroes CSS custom properties, which is
// enough for CSS-only transitions and the JS-driven `transition:` directives that read them via
// `motionFast()`/`motionBase()`. Anything driven by its own JS loop (e.g. a canvas particle
// animation) instead needs a plain boolean to decide whether to run at all — this combines both
// signals (`.private/feature/reduce-motion-setting/PLAN.md`'s "setting OR OS preference"
// formula) for that case. Defaults to `false` ("don't assume reduced") when the environment can't
// tell, same defensive posture as `readMs`'s fallback.
export function prefersReducedMotion(): boolean {
  if (typeof document === "undefined") return false;
  if (document.documentElement.hasAttribute("data-reduce-motion")) return true;
  if (typeof matchMedia !== "function") return false;
  return matchMedia("(prefers-reduced-motion: reduce)").matches;
}
