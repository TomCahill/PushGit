// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Fixed set of built-in theme presets — see `.private/feature/theme-presets/PLAN.md`. Kept as
// a plain string on the wire (see `AppConfig.theme` in `git/types.ts`), not a union there, so
// an unrecognized value round-trips harmlessly; this file is the single source of truth for
// which ids are actually offered in the UI.
export type ThemeId = "default" | "solarized" | "github";

export const THEMES: { id: ThemeId; label: string }[] = [
  { id: "default", label: "Default" },
  { id: "solarized", label: "Solarized" },
  { id: "github", label: "GitHub" },
];
