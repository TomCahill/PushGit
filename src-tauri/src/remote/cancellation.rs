// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-repo cancellation tokens for in-flight fetch/pull/push — part of the
//! "cancellation is built in from day one" requirement.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

#[derive(Default)]
pub struct CancellationRegistry {
    tokens: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl CancellationRegistry {
    /// Registers a fresh cancellation token for `repo_path`, returning it so the caller can
    /// check it during the operation it's about to run. Replaces (and thereby orphans) any
    /// token already registered for the same path — a stale cancel request for a finished
    /// operation can never reach back and affect a newer one at the same path.
    pub async fn register(&self, repo_path: &Path) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        self.tokens
            .lock()
            .await
            .insert(repo_path.to_string_lossy().into_owned(), token.clone());
        token
    }

    /// Requests cancellation of whatever's currently registered for `repo_path`. A no-op if
    /// nothing is registered (the operation already finished, or was never started).
    pub async fn cancel(&self, repo_path: &Path) {
        let tokens = self.tokens.lock().await;
        if let Some(token) = tokens.get(repo_path.to_string_lossy().as_ref()) {
            token.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_returns_a_fresh_unset_token() {
        let registry = CancellationRegistry::default();

        let token = registry.register(Path::new("/repo")).await;

        assert!(!token.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancel_sets_the_registered_token() {
        let registry = CancellationRegistry::default();
        let token = registry.register(Path::new("/repo")).await;

        registry.cancel(Path::new("/repo")).await;

        assert!(token.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancel_is_a_no_op_for_an_unregistered_path() {
        let registry = CancellationRegistry::default();

        registry.cancel(Path::new("/never/registered")).await; // must not panic
    }

    #[tokio::test]
    async fn registering_again_orphans_the_previous_token() {
        let registry = CancellationRegistry::default();
        let first = registry.register(Path::new("/repo")).await;
        let second = registry.register(Path::new("/repo")).await;

        registry.cancel(Path::new("/repo")).await;

        assert!(!first.load(Ordering::Relaxed));
        assert!(second.load(Ordering::Relaxed));
    }
}
