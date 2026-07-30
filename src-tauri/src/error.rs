// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unified error enum mapping git2, I/O, and subprocess errors to a single serializable
//! error shape for the frontend.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum PushGitError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("subprocess `{command}` failed: {message}")]
    Subprocess { command: String, message: String },

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Invalid(String),
}

/// Tauri serializes command `Err` values as JSON; `thiserror`'s `Display` output alone
/// isn't structured enough for the frontend to branch on error kind, so this gives each
/// variant a stable string tag instead of relying on message-text matching.
impl Serialize for PushGitError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type PushGitResult<T> = Result<T, PushGitError>;
