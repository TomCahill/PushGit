// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Managed local AI inference: downloading and verifying the pinned model + engine, running
//! `llama-server` as a long-lived subprocess, and reporting download/readiness status. See
//! `ai::openai` for the actual generation request once the engine is up — this module only
//! covers acquiring and running it, deliberately reusing that existing adapter rather than
//! adding a new one.

mod cancellation;
mod download;
mod engine;
mod manifest;

pub use cancellation::LocalAiCancellation;
pub use download::{download_local_ai, DownloadProgress};
pub use engine::{ensure_local_engine_running, LocalEngineHandle};

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{PushGitError, PushGitResult};

/// The exact model this feature manages — Qwen2.5-Coder-1.5B-Instruct, `Q4_K_M` GGUF,
/// Apache-2.0 licensed (verified on its Hugging Face model card). Re-pinning to a newer
/// quantization or model is a deliberate version bump, never "always fetch latest."
pub const MODEL_URL: &str = "https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf";
pub const MODEL_SHA256: &str = "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046";
pub const MODEL_SIZE: u64 = 1_117_320_768;

/// The exact llama.cpp release this feature manages — the CPU-only Linux x86_64 build; the
/// Vulkan/ROCm/SYCL assets published at the same release are deliberately not used (see the
/// feature plan's "Decided scope" — GPU acceleration is out of scope this pass).
pub const ENGINE_URL: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download/b10203/llama-b10203-bin-ubuntu-x64.tar.gz";
pub const ENGINE_SHA256: &str = "cd1606ab9380ec5be48bb8eae4003e96d5c87f0a0bb4cad351c15248f121135b";
pub const ENGINE_SIZE: u64 = 16_430_342;
/// The archive's own top-level directory, stripped during extraction so every extracted file
/// lands directly under `local_ai_dir()/engine/` regardless of which build is pinned.
const ENGINE_ARCHIVE_TOP_DIR: &str = "llama-b10203";
const ENGINE_BINARY_NAME: &str = "llama-server";

fn resolve_dir() -> PushGitResult<PathBuf> {
    crate::config::local_ai_dir().ok_or_else(|| {
        PushGitError::Invalid("could not resolve the local-AI data directory".to_string())
    })
}

fn model_path(dir: &Path) -> PathBuf {
    dir.join("model.gguf")
}

fn engine_dir(dir: &Path) -> PathBuf {
    dir.join("engine")
}

fn engine_binary_path(dir: &Path) -> PathBuf {
    engine_dir(dir).join(ENGINE_BINARY_NAME)
}

/// Whether the pinned model/engine are downloaded, extracted, and verified against the current
/// pin — not a bare file-existence check, so a stale file from an older pin correctly reads as
/// absent rather than silently passing as ready (see `manifest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalAiStatus {
    pub model_present: bool,
    pub engine_present: bool,
}

pub fn get_local_ai_status() -> LocalAiStatus {
    match resolve_dir() {
        Ok(dir) => status_from(&dir),
        Err(_) => LocalAiStatus::default(),
    }
}

fn status_from(dir: &Path) -> LocalAiStatus {
    let manifest = manifest::load(dir);
    LocalAiStatus {
        model_present: manifest.model_checksum.as_deref() == Some(MODEL_SHA256)
            && model_path(dir).is_file(),
        engine_present: manifest.engine_checksum.as_deref() == Some(ENGINE_SHA256)
            && engine_binary_path(dir).is_file(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn nothing_present_when_the_directory_is_empty() {
        let dir = TempDir::new().unwrap();

        assert_eq!(status_from(dir.path()), LocalAiStatus::default());
    }

    #[test]
    fn model_present_when_the_manifest_checksum_matches_and_the_file_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::write(model_path(dir.path()), b"fake model bytes").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some(MODEL_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(status_from(dir.path()).model_present);
    }

    #[test]
    fn model_not_present_when_the_manifest_checksum_is_stale() {
        let dir = TempDir::new().unwrap();
        std::fs::write(model_path(dir.path()), b"fake model bytes from an old pin").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some("some-older-pinned-checksum".to_string());
        manifest::save(dir.path(), &manifest);

        assert!(!status_from(dir.path()).model_present);
    }

    #[test]
    fn model_not_present_when_the_file_is_missing_despite_a_matching_manifest() {
        let dir = TempDir::new().unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some(MODEL_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(!status_from(dir.path()).model_present);
    }

    #[test]
    fn engine_present_when_the_manifest_checksum_matches_and_the_binary_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(engine_dir(dir.path())).unwrap();
        std::fs::write(engine_binary_path(dir.path()), b"fake binary").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.engine_checksum = Some(ENGINE_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(status_from(dir.path()).engine_present);
    }
}
