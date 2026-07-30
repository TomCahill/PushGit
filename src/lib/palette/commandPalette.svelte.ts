// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Imperative, singleton command-palette open/close state, architected exactly like
// `confirmDialog.svelte.ts`/`contextMenu.svelte.ts`. This store only tracks whether the
// palette is open — the searchable data (commands/branches/files/commits) lives in
// `CommandPalette.svelte` itself, since fetching it needs `repoPath`, which this
// repo-agnostic store deliberately doesn't know about.

export type PaletteCommand = {
  id: string;
  label: string;
  run: () => void | Promise<void>;
};

export const paletteState: { open: boolean } = $state({ open: false });

export function openCommandPalette(): void {
  paletteState.open = true;
}

export function closeCommandPalette(): void {
  paletteState.open = false;
}
