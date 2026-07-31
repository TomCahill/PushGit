// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tauri `State<T>` for shared repo handles and caches, held behind `tokio::sync::Mutex`/
//! `RwLock` (not `std::sync::Mutex`, which is not `Send`-safe across `.await` points).

use notify_debouncer_full::{notify::RecommendedWatcher, Debouncer, RecommendedCache};
use tokio::sync::Mutex;

use crate::graph::GraphSessions;
use crate::remote::CancellationRegistry;
use crate::undo::UndoLog;

#[derive(Default)]
pub struct AppState {
    pub graph_sessions: GraphSessions,
    /// The single repo-directory watcher for the currently open repo, if any. Starting a
    /// new watch (or closing the repo) replaces/clears this, which stops the previous one.
    pub repo_watcher: Mutex<Option<Debouncer<RecommendedWatcher, RecommendedCache>>>,
    /// Cancellation tokens for in-flight fetch/pull/push, keyed by repo path.
    pub remote_cancellation: CancellationRegistry,
    /// Cancellation tokens for in-flight AI commit-message generation, keyed by repo path — a
    /// separate instance from `remote_cancellation`: that registry keys purely by repo path, so
    /// sharing it would let a concurrent fetch/pull/push and AI generation on the same repo
    /// orphan each other's cancel token.
    pub ai_cancellation: CancellationRegistry,
    /// Undo/redo history for destructive operations, keyed by repo path.
    pub undo_log: UndoLog,
    /// The repo path passed on the command line at cold start (`tauri-plugin-cli`),
    /// if any. Read once via
    /// `commands::get_startup_repo_path`, which `take()`s it so a later call (e.g. a page
    /// reload) doesn't keep re-opening the same repo. Plain `std::sync::Mutex`, not
    /// `tokio::sync::Mutex` — set once synchronously in `.setup()`, read once synchronously,
    /// never held across an `.await`.
    pub startup_repo_path: std::sync::Mutex<Option<String>>,
}
