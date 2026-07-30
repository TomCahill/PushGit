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
