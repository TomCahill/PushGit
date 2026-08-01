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

use serde::{Deserialize, Serialize};

use crate::error::{PushGitError, PushGitResult};

/// The exact model this feature manages — Qwen2.5-Coder-1.5B-Instruct, `Q4_K_M` GGUF,
/// Apache-2.0 licensed (verified on its Hugging Face model card). Re-pinning to a newer
/// quantization or model is a deliberate version bump, never "always fetch latest." Shared by
/// every engine variant — only the engine binary differs between CPU and GPU acceleration.
pub const MODEL_URL: &str = "https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf";
pub const MODEL_SHA256: &str = "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046";
pub const MODEL_SIZE: u64 = 1_117_320_768;

/// The CPU-only Linux x86_64 build of the pinned llama.cpp release — guaranteed to work on any
/// hardware, so this is the default `EngineVariant`.
pub const ENGINE_CPU_URL: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download/b10203/llama-b10203-bin-ubuntu-x64.tar.gz";
pub const ENGINE_CPU_SHA256: &str =
    "cd1606ab9380ec5be48bb8eae4003e96d5c87f0a0bb4cad351c15248f121135b";
pub const ENGINE_CPU_SIZE: u64 = 16_430_342;

/// The Vulkan Linux x86_64 build of the same pinned release — an opt-in second variant, never
/// auto-selected, covering NVIDIA/AMD/Intel generically through whatever Vulkan driver the user
/// already has (see the feature plan's "Why Vulkan, not ROCm/CUDA/SYCL").
pub const ENGINE_VULKAN_URL: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download/b10203/llama-b10203-bin-ubuntu-vulkan-x64.tar.gz";
pub const ENGINE_VULKAN_SHA256: &str =
    "49329bccd12d5e3abf96e8cea49492fa0f247fb699cd551af529a1005a7593ac";
pub const ENGINE_VULKAN_SIZE: u64 = 32_416_251;

/// The archive's own top-level directory, stripped during extraction so every extracted file
/// lands directly under `local_ai_dir()/engine/<variant>/` regardless of which build is pinned.
/// Shared by both variants — same release, same archive layout.
const ENGINE_ARCHIVE_TOP_DIR: &str = "llama-b10203";
const ENGINE_BINARY_NAME: &str = "llama-server";

/// Which `llama-server` build is downloaded/run. CPU is the default and the only variant
/// guaranteed to work everywhere; Vulkan is an explicit opt-in for GPU offload. Each variant
/// gets its own subdirectory under `local_ai_dir()/engine/` and its own manifest checksum
/// fields, so switching back and forth after both are downloaded never re-downloads either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EngineVariant {
    #[default]
    Cpu,
    Vulkan,
}

impl EngineVariant {
    fn engine_url(self) -> &'static str {
        match self {
            EngineVariant::Cpu => ENGINE_CPU_URL,
            EngineVariant::Vulkan => ENGINE_VULKAN_URL,
        }
    }

    fn engine_sha256(self) -> &'static str {
        match self {
            EngineVariant::Cpu => ENGINE_CPU_SHA256,
            EngineVariant::Vulkan => ENGINE_VULKAN_SHA256,
        }
    }

    fn engine_size(self) -> u64 {
        match self {
            EngineVariant::Cpu => ENGINE_CPU_SIZE,
            EngineVariant::Vulkan => ENGINE_VULKAN_SIZE,
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            EngineVariant::Cpu => "cpu",
            EngineVariant::Vulkan => "vulkan",
        }
    }
}

fn resolve_dir() -> PushGitResult<PathBuf> {
    crate::config::local_ai_dir().ok_or_else(|| {
        PushGitError::Invalid("could not resolve the local-AI data directory".to_string())
    })
}

fn model_path(dir: &Path) -> PathBuf {
    dir.join("model.gguf")
}

fn engine_dir(dir: &Path, variant: EngineVariant) -> PathBuf {
    dir.join("engine").join(variant.dir_name())
}

fn engine_binary_path(dir: &Path, variant: EngineVariant) -> PathBuf {
    engine_dir(dir, variant).join(ENGINE_BINARY_NAME)
}

fn model_verified(dir: &Path) -> bool {
    manifest::load(dir).model_checksum.as_deref() == Some(MODEL_SHA256) && model_path(dir).is_file()
}

fn engine_verified(dir: &Path, variant: EngineVariant) -> bool {
    manifest::load(dir).engine_checksum(variant) == Some(variant.engine_sha256())
        && engine_binary_path(dir, variant).is_file()
}

/// Whether the pinned model/engine are downloaded, extracted, and verified against the current
/// pin — not a bare file-existence check, so a stale file from an older pin correctly reads as
/// absent rather than silently passing as ready (see `manifest`). `engine_present`/`gpu_device`
/// are reported for whichever `EngineVariant` is asked about — the Settings UI asks about the
/// *drafted* selection directly, so previewing a variant never requires saving it first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalAiStatus {
    pub model_present: bool,
    pub engine_present: bool,
    /// The GPU device `llama-server --list-devices` reports, populated only when `variant` is
    /// `Vulkan` and its engine is present — reassurance ("GPU: NVIDIA GeForce RTX 3060") when
    /// `Some`, or an explicit "no Vulkan-capable GPU detected" warning in the UI when `None`,
    /// rather than the user silently getting CPU speed while believing they'd opted into GPU.
    pub gpu_device: Option<String>,
}

pub async fn get_local_ai_status(variant: EngineVariant) -> LocalAiStatus {
    match resolve_dir() {
        Ok(dir) => status_from(&dir, variant).await,
        Err(_) => LocalAiStatus::default(),
    }
}

async fn status_from(dir: &Path, variant: EngineVariant) -> LocalAiStatus {
    let model_present = model_verified(dir);
    let engine_present = engine_verified(dir, variant);
    let gpu_device = if variant == EngineVariant::Vulkan && engine_present {
        engine::detect_gpu_device(dir, variant).await
    } else {
        None
    };
    LocalAiStatus {
        model_present,
        engine_present,
        gpu_device,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn nothing_present_when_the_directory_is_empty() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            status_from(dir.path(), EngineVariant::Cpu).await,
            LocalAiStatus::default()
        );
    }

    #[tokio::test]
    async fn model_present_when_the_manifest_checksum_matches_and_the_file_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::write(model_path(dir.path()), b"fake model bytes").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some(MODEL_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(
            status_from(dir.path(), EngineVariant::Cpu)
                .await
                .model_present
        );
    }

    #[tokio::test]
    async fn model_not_present_when_the_manifest_checksum_is_stale() {
        let dir = TempDir::new().unwrap();
        std::fs::write(model_path(dir.path()), b"fake model bytes from an old pin").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some("some-older-pinned-checksum".to_string());
        manifest::save(dir.path(), &manifest);

        assert!(
            !status_from(dir.path(), EngineVariant::Cpu)
                .await
                .model_present
        );
    }

    #[tokio::test]
    async fn model_not_present_when_the_file_is_missing_despite_a_matching_manifest() {
        let dir = TempDir::new().unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some(MODEL_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(
            !status_from(dir.path(), EngineVariant::Cpu)
                .await
                .model_present
        );
    }

    #[tokio::test]
    async fn engine_present_when_the_manifest_checksum_matches_and_the_binary_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(engine_dir(dir.path(), EngineVariant::Cpu)).unwrap();
        std::fs::write(
            engine_binary_path(dir.path(), EngineVariant::Cpu),
            b"fake binary",
        )
        .unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.set_engine_checksum(EngineVariant::Cpu, Some(ENGINE_CPU_SHA256.to_string()));
        manifest::save(dir.path(), &manifest);

        assert!(
            status_from(dir.path(), EngineVariant::Cpu)
                .await
                .engine_present
        );
    }

    #[tokio::test]
    async fn engine_variants_are_tracked_independently() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(engine_dir(dir.path(), EngineVariant::Cpu)).unwrap();
        std::fs::write(
            engine_binary_path(dir.path(), EngineVariant::Cpu),
            b"fake cpu binary",
        )
        .unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.set_engine_checksum(EngineVariant::Cpu, Some(ENGINE_CPU_SHA256.to_string()));
        manifest::save(dir.path(), &manifest);

        assert!(
            status_from(dir.path(), EngineVariant::Cpu)
                .await
                .engine_present
        );
        assert!(
            !status_from(dir.path(), EngineVariant::Vulkan)
                .await
                .engine_present
        );
    }

    #[tokio::test]
    async fn gpu_device_is_never_probed_for_the_cpu_variant() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(engine_dir(dir.path(), EngineVariant::Cpu)).unwrap();
        std::fs::write(
            engine_binary_path(dir.path(), EngineVariant::Cpu),
            b"fake cpu binary",
        )
        .unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.set_engine_checksum(EngineVariant::Cpu, Some(ENGINE_CPU_SHA256.to_string()));
        manifest::save(dir.path(), &manifest);

        assert_eq!(
            status_from(dir.path(), EngineVariant::Cpu).await.gpu_device,
            None
        );
    }
}
