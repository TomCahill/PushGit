// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `local_ai_dir()/manifest.json` — records which pinned checksum (if any) the currently
//! downloaded model/engine files were verified against, and which pin a `.part` file in
//! progress is downloading toward. Makes status and download-resume version-aware instead of
//! trusting bare file existence.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Set once the downloaded model file's checksum has verified against the current pin.
    pub model_checksum: Option<String>,
    /// The pin a `model.gguf.part` in progress is downloading toward — checked before resuming
    /// it, so a `.part` left over from a since-changed pin is discarded rather than resumed
    /// into a corrupt splice of two different versions.
    pub model_pending_checksum: Option<String>,
    pub engine_checksum: Option<String>,
    pub engine_pending_checksum: Option<String>,
}

fn manifest_path(dir: &Path) -> std::path::PathBuf {
    dir.join("manifest.json")
}

/// Missing/corrupt manifest degrades to `Manifest::default()` — matches `config`'s established
/// "a settings-storage problem should never block the app" philosophy; worst case, a fresh
/// download is triggered instead of a resume.
pub fn load(dir: &Path) -> Manifest {
    std::fs::read_to_string(manifest_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Best-effort — a write failure here just means the next status check/resume attempt falls
/// back to treating nothing as present, never a hard error surfaced mid-download.
pub fn save(dir: &Path, manifest: &Manifest) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(manifest) {
        let _ = std::fs::write(manifest_path(dir), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_manifest_loads_as_default() {
        let dir = TempDir::new().unwrap();

        assert_eq!(load(dir.path()), Manifest::default());
    }

    #[test]
    fn corrupt_manifest_loads_as_default() {
        let dir = TempDir::new().unwrap();
        std::fs::write(manifest_path(dir.path()), "not json").unwrap();

        assert_eq!(load(dir.path()), Manifest::default());
    }

    #[test]
    fn manifest_round_trips_through_a_save_and_load() {
        let dir = TempDir::new().unwrap();
        let manifest = Manifest {
            model_checksum: Some("abc".to_string()),
            model_pending_checksum: None,
            engine_checksum: None,
            engine_pending_checksum: Some("def".to_string()),
        };

        save(dir.path(), &manifest);

        assert_eq!(load(dir.path()), manifest);
    }
}
