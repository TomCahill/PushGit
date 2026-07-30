// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Imperative, singleton right-click menu, rendered through the shared <ContextMenu /> mounted
// once at the shell root (`+page.svelte`) — architected exactly like `confirmDialog.svelte.ts`'s
// confirmAsync()/promptAsync(). Unlike those, openContextMenu() returns nothing: each item owns
// its own onSelect callback instead of the caller awaiting a single result.

export type ContextMenuAction = {
  label: string;
  onSelect: () => void;
  disabled?: boolean;
  danger?: boolean;
};
export type ContextMenuSeparator = { separator: true };
export type ContextMenuItem = ContextMenuAction | ContextMenuSeparator;

type ContextMenuRequest = {
  x: number;
  y: number;
  items: ContextMenuItem[];
};

export const menuState: { request: ContextMenuRequest | null } = $state({ request: null });

export function openContextMenu(x: number, y: number, items: ContextMenuItem[]): void {
  menuState.request = { x, y, items };
}

/** Callers that open a menu against a specific subject (a repo path, a session) must call this
 *  when that subject goes away — e.g. `CommitGraph.svelte`'s `reset()` on a repo/filter switch —
 *  otherwise an already-open menu's item closures keep pointing at the stale subject. */
export function closeContextMenu(): void {
  menuState.request = null;
}
