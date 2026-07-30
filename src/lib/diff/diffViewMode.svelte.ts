// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Shared, in-memory-only "inline vs side-by-side" diff view preference
// ("the choice is remembered per session"), read and written by every `HunkDiff` instance
// across the app so switching it in one file/commit view carries over to the next. Same
// "stable within a session, reset on restart" scope as the graph's color palette
// (`src/lib/graph/palette.ts`'s header comment) — no persistence beyond the running app.
export const diffViewState: { mode: "inline" | "side-by-side" } = $state({ mode: "inline" });
