// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! OS-keyring-backed storage for the AI feature's API key — a single fixed service/username
//! slot, never `config.json` (`config::AppConfig` holds every other AI setting). Unlike that
//! module's silent-degrade-on-failure philosophy, a save/clear failure here is surfaced to the
//! caller: a key that silently failed to save would only surface later as a confusing
//! "works in Settings, fails when generating" failure. A read failure (locked keyring, no
//! Secret Service provider running, or simply nothing saved yet) still degrades to `None` —
//! from the caller's perspective that's indistinguishable from "no key configured," the same
//! state as a fresh install.

use crate::error::{PushGitError, PushGitResult};

const SERVICE: &str = "pushgit";
const USERNAME: &str = "ai-api-key";

fn entry() -> PushGitResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, USERNAME)
        .map_err(|e| PushGitError::Invalid(format!("keyring unavailable: {e}")))
}

pub fn store_api_key(key: &str) -> PushGitResult<()> {
    entry()?
        .set_password(key)
        .map_err(|e| PushGitError::Invalid(format!("failed to save API key: {e}")))
}

pub fn get_api_key() -> Option<String> {
    entry().ok()?.get_password().ok()
}

pub fn has_api_key() -> bool {
    get_api_key().is_some()
}

/// A no-op if nothing was ever stored (`NoEntry`) — matches
/// `CancellationRegistry::cancel`'s "cancelling nothing is fine" precedent elsewhere in this
/// codebase.
pub fn clear_api_key() -> PushGitResult<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(PushGitError::Invalid(format!(
            "failed to clear API key: {e}"
        ))),
    }
}
