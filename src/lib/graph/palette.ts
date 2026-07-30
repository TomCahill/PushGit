// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// The commit graph's color palette is frontend-owned — the backend only ever sends the
// small integer `colorId`, so re-theming never
// needs a backend round-trip. Each entry is a light/dark pair fed through CSS
// `light-dark()`, which resolves automatically from the OS color-scheme with no JS
// theme-detection needed (see `:root { color-scheme: light dark }` in `+page.svelte`).

const PALETTE: readonly [light: string, dark: string][] = [
  ["#B25000", "#E69F00"], // orange
  ["#0072B2", "#56B4E9"], // blue
  ["#00785A", "#009E73"], // green
  ["#8A7300", "#F0E442"], // yellow
  ["#00539C", "#5FA8E0"], // deep blue
  ["#A3390A", "#D55E00"], // vermillion
  ["#A0447D", "#CC79A7"], // pink
  ["#00787A", "#00C2C4"], // teal
];

/** A stable CSS color string for a given `colorId`, cycling if it exceeds the palette. */
export function colorFor(colorId: number): string {
  const [light, dark] = PALETTE[colorId % PALETTE.length];
  return `light-dark(${light}, ${dark})`;
}
