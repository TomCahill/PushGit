// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Imperative confirm()/prompt() replacement, rendered through the shared, themed
// <ConfirmDialog /> mounted once at the shell root (`+page.svelte`) — the browser's native
// confirm()/prompt() don't respect the app's light/dark theme, which looks out of place
// next to everything else. Call confirmAsync()/promptAsync() exactly like the natives they
// replace: await it, act on the result.

type ConfirmRequest = {
  kind: "confirm";
  message: string;
  resolve: (result: boolean) => void;
};

type PromptRequest = {
  kind: "prompt";
  message: string;
  defaultValue: string;
  resolve: (result: string | null) => void;
};

export const dialogState: { request: ConfirmRequest | PromptRequest | null } = $state({
  request: null,
});

export function confirmAsync(message: string): Promise<boolean> {
  return new Promise((resolve) => {
    dialogState.request = { kind: "confirm", message, resolve };
  });
}

export function promptAsync(message: string, defaultValue = ""): Promise<string | null> {
  return new Promise((resolve) => {
    dialogState.request = { kind: "prompt", message, defaultValue, resolve };
  });
}
