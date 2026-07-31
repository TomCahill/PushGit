// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A singleton cancellation token for the local-AI model/engine download — deliberately not
//! `remote::CancellationRegistry`, which is keyed by repo path for concurrent per-repo
//! operations. A download is a single, app-level, at-most-one-at-a-time operation, so this
//! holds at most one token rather than a map.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::{PushGitError, PushGitResult};

#[derive(Default)]
pub struct LocalAiCancellation {
    token: Mutex<Option<Arc<AtomicBool>>>,
}

impl LocalAiCancellation {
    /// Registers a fresh token for a new download, failing if one is already registered —
    /// doubling as the guard against two concurrent downloads racing on the same `.part` files.
    pub async fn register(&self) -> PushGitResult<Arc<AtomicBool>> {
        let mut slot = self.token.lock().await;
        if slot.is_some() {
            return Err(PushGitError::Invalid(
                "a local AI download is already in progress".to_string(),
            ));
        }
        let token = Arc::new(AtomicBool::new(false));
        *slot = Some(token.clone());
        Ok(token)
    }

    /// Requests cancellation of whatever download is currently in progress, if any — a no-op
    /// if nothing is.
    pub async fn cancel(&self) {
        if let Some(token) = self.token.lock().await.as_ref() {
            token.store(true, Ordering::Relaxed);
        }
    }

    /// Clears the registered token once a download finishes (success, failure, or
    /// cancellation) — must be called so a later download isn't rejected by the guard above.
    pub async fn clear(&self) {
        *self.token.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_returns_a_fresh_unset_token() {
        let cancellation = LocalAiCancellation::default();

        let token = cancellation.register().await.unwrap();

        assert!(!token.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn a_second_register_is_rejected_while_one_is_in_progress() {
        let cancellation = LocalAiCancellation::default();
        let _first = cancellation.register().await.unwrap();

        let second = cancellation.register().await;

        assert!(second.is_err());
    }

    #[tokio::test]
    async fn clear_allows_a_new_download_to_register() {
        let cancellation = LocalAiCancellation::default();
        let _first = cancellation.register().await.unwrap();
        cancellation.clear().await;

        let second = cancellation.register().await;

        assert!(second.is_ok());
    }

    #[tokio::test]
    async fn cancel_sets_the_registered_token() {
        let cancellation = LocalAiCancellation::default();
        let token = cancellation.register().await.unwrap();

        cancellation.cancel().await;

        assert!(token.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancel_is_a_no_op_when_nothing_is_registered() {
        let cancellation = LocalAiCancellation::default();

        cancellation.cancel().await; // must not panic
    }
}
