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

// The UI gets the bare message; `Display` keeps the prefix and libgit2's class/code for debugging.
impl Serialize for PushGitError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.user_message())
    }
}

impl PushGitError {
    fn user_message(&self) -> String {
        match self {
            PushGitError::Git(err) => capitalize_first(err.message()),
            PushGitError::Io(err) => err.to_string(),
            other => other.to_string(),
        }
    }
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub type PushGitResult<T> = Result<T, PushGitError>;

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{ErrorClass, ErrorCode};

    fn serialized(err: &PushGitError) -> String {
        serde_json::to_value(err)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn not_found_ref() -> PushGitError {
        PushGitError::Git(git2::Error::new(
            ErrorCode::NotFound,
            ErrorClass::Reference,
            "cannot locate remote-tracking branch 'origin/x'",
        ))
    }

    #[test]
    fn git_errors_reach_the_ui_without_prefix_or_class_and_code() {
        assert_eq!(
            serialized(&not_found_ref()),
            "Cannot locate remote-tracking branch 'origin/x'"
        );
    }

    #[test]
    fn git_display_keeps_debug_detail() {
        let text = not_found_ref().to_string();
        assert!(text.starts_with("git error: cannot locate"));
        assert!(text.contains("class=Reference"));
        assert!(text.contains("code=NotFound"));
    }

    #[test]
    fn io_errors_drop_the_prefix() {
        let err = PushGitError::Io(std::io::Error::other("disk full"));
        assert_eq!(serialized(&err), "disk full");
    }

    #[test]
    fn cancelled_text_is_unchanged_for_frontend_matching() {
        assert_eq!(serialized(&PushGitError::Cancelled), "operation cancelled");
    }
}
