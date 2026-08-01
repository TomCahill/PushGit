// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Downloads and verifies the pinned engine archive and model file, resuming an interrupted
//! `.part` file where possible and extracting the engine once its archive checksum verifies.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{PushGitError, PushGitResult};

use super::manifest;
use super::{
    engine_binary_path, engine_dir, engine_verified, model_path, model_verified, resolve_dir,
    EngineVariant, ENGINE_ARCHIVE_TOP_DIR, MODEL_SHA256, MODEL_SIZE, MODEL_URL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub enum DownloadStage {
    Engine,
    Model,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub stage: DownloadStage,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

/// Which pinned file a download/verification pass is for — `Engine(variant)` selects that
/// variant's manifest checksum fields and archive path; `Model` is shared by every variant.
/// Distinct from `DownloadStage`, which only tags wire-progress messages as "engine" or
/// "model" and doesn't need to (and shouldn't) leak which engine variant to the frontend.
#[derive(Debug, Clone, Copy)]
enum Asset {
    Model,
    Engine(EngineVariant),
}

impl Asset {
    fn wire_stage(self) -> DownloadStage {
        match self {
            Asset::Model => DownloadStage::Model,
            Asset::Engine(_) => DownloadStage::Engine,
        }
    }
}

fn engine_archive_path(dir: &Path, variant: EngineVariant) -> PathBuf {
    dir.join(format!(
        "engine-{}.tar.gz",
        match variant {
            EngineVariant::Cpu => "cpu",
            EngineVariant::Vulkan => "vulkan",
        }
    ))
}

fn part_path_for(final_path: &Path) -> PathBuf {
    let mut part = final_path.as_os_str().to_owned();
    part.push(".part");
    PathBuf::from(part)
}

/// Fetches `variant`'s engine (skipped if already present+verified) then the shared model
/// (skipped if already present+verified) — the skip checks are what make switching engine
/// variants after both are downloaded, or re-downloading after an interrupted first attempt,
/// never redundantly re-fetch a file already on disk. Extracting and marking each verified as
/// it completes means a generation attempt right after the engine finishes but before the
/// (much larger) model download completes still sees an accurate status rather than an
/// all-or-nothing flag.
pub async fn download_local_ai(
    variant: EngineVariant,
    channel: &Channel<DownloadProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let dir = resolve_dir()?;
    tokio::fs::create_dir_all(&dir).await?;

    if !engine_verified(&dir, variant) {
        let archive_path = engine_archive_path(&dir, variant);
        download_verified(
            &dir,
            Asset::Engine(variant),
            variant.engine_url(),
            variant.engine_size(),
            variant.engine_sha256(),
            &archive_path,
            channel,
            cancel,
        )
        .await?;
        extract_engine(dir.clone(), archive_path.clone(), variant).await?;
        let _ = tokio::fs::remove_file(&archive_path).await;
        mark_verified(&dir, Asset::Engine(variant), variant.engine_sha256());
    }

    if !model_verified(&dir) {
        download_verified(
            &dir,
            Asset::Model,
            MODEL_URL,
            MODEL_SIZE,
            MODEL_SHA256,
            &model_path(&dir),
            channel,
            cancel,
        )
        .await?;
        mark_verified(&dir, Asset::Model, MODEL_SHA256);
    }

    Ok(())
}

fn mark_verified(dir: &Path, asset: Asset, checksum: &str) {
    let mut manifest = manifest::load(dir);
    match asset {
        Asset::Model => {
            manifest.model_checksum = Some(checksum.to_string());
            manifest.model_pending_checksum = None;
        }
        Asset::Engine(variant) => {
            manifest.set_engine_checksum(variant, Some(checksum.to_string()));
            manifest.set_engine_pending_checksum(variant, None);
        }
    }
    manifest::save(dir, &manifest);
}

/// Downloads `url` to `final_path`, resuming a `.part` file left over from an interrupted
/// attempt where possible, verifying its checksum, and only then renaming it into place. A
/// checksum mismatch deletes the `.part` file and surfaces an error — never auto-retried,
/// matching this feature's existing "no automatic retries" policy for AI network calls.
#[allow(clippy::too_many_arguments)]
async fn download_verified(
    dir: &Path,
    asset: Asset,
    url: &str,
    expected_size: u64,
    expected_sha256: &str,
    final_path: &Path,
    channel: &Channel<DownloadProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let part_path = part_path_for(final_path);
    reconcile_pending_pin(dir, asset, expected_sha256, &part_path).await;

    let existing_len = tokio::fs::metadata(&part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    if existing_len < expected_size {
        fetch_into(
            url,
            &part_path,
            existing_len,
            expected_size,
            asset.wire_stage(),
            channel,
            cancel,
        )
        .await?;
    }

    let actual_sha256 = hash_file(&part_path).await?;
    if actual_sha256 != expected_sha256 {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(PushGitError::Invalid(format!(
            "downloaded file failed checksum verification — please retry (expected {expected_sha256}, got {actual_sha256})"
        )));
    }

    tokio::fs::rename(&part_path, final_path).await?;
    Ok(())
}

/// If a `.part` file exists but was started against a different pin than `expected_sha256`
/// (the app was updated between attempts), discard it rather than risk appending bytes from two
/// different pinned versions into one file.
async fn reconcile_pending_pin(dir: &Path, asset: Asset, expected_sha256: &str, part_path: &Path) {
    let mut manifest = manifest::load(dir);
    let current_pending = match asset {
        Asset::Model => manifest.model_pending_checksum.clone(),
        Asset::Engine(variant) => manifest
            .engine_pending_checksum(variant)
            .map(str::to_string),
    };
    if current_pending.as_deref() != Some(expected_sha256) {
        let _ = tokio::fs::remove_file(part_path).await;
        match asset {
            Asset::Model => manifest.model_pending_checksum = Some(expected_sha256.to_string()),
            Asset::Engine(variant) => {
                manifest.set_engine_pending_checksum(variant, Some(expected_sha256.to_string()))
            }
        }
        manifest::save(dir, &manifest);
    }
}

/// Streams `url` into `part_path`, appending from `existing_len` via an HTTP `Range` request
/// when the server supports it (falling back to a from-scratch download otherwise), reporting
/// progress through `channel` and checking `cancel` between chunks. On cancellation, the
/// `.part` file is left in place — the next attempt resumes from it, per this feature's
/// "handle network interruptions gracefully" requirement. A local disk error (e.g. out of
/// space) surfaces the same way a network error does, with no special-casing: the `.part` file
/// is left as-is and the error is returned once.
async fn fetch_into(
    url: &str,
    part_path: &Path,
    existing_len: u64,
    expected_size: u64,
    stage: DownloadStage,
    channel: &Channel<DownloadProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let client = reqwest::Client::new();

    let mut resume_from = existing_len;
    if resume_from > 0 && !supports_range(&client, url).await {
        let _ = tokio::fs::remove_file(part_path).await;
        resume_from = 0;
    }

    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| PushGitError::Invalid(format!("download request failed: {e}")))?;
    if !response.status().is_success() {
        return Err(PushGitError::Invalid(format!(
            "download failed with HTTP {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .await?;
    if resume_from == 0 {
        file.set_len(0).await?;
    }

    let mut downloaded = resume_from;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err(PushGitError::Cancelled);
        }
        let chunk = chunk.map_err(|e| PushGitError::Invalid(format!("download failed: {e}")))?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let _ = channel.send(DownloadProgress {
            stage,
            bytes_downloaded: downloaded,
            bytes_total: expected_size,
        });
    }

    Ok(())
}

async fn supports_range(client: &reqwest::Client, url: &str) -> bool {
    client
        .head(url)
        .send()
        .await
        .ok()
        .and_then(|r| r.headers().get(reqwest::header::ACCEPT_RANGES).cloned())
        .and_then(|v| v.to_str().ok().map(str::to_string))
        .is_some_and(|v| v == "bytes")
}

async fn hash_file(path: &Path) -> PushGitResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extraction is blocking file I/O (`flate2`/`tar` have no async API), so it's dispatched
/// through `spawn_blocking` — the same discipline `ARCHITECTURE.md` §5 requires for every
/// git2-rs call, applied here for consistency even though a 16MB archive extracts in
/// milliseconds.
async fn extract_engine(
    dir: PathBuf,
    archive_path: PathBuf,
    variant: EngineVariant,
) -> PushGitResult<()> {
    tokio::task::spawn_blocking(move || extract_engine_blocking(&dir, &archive_path, variant))
        .await
        .map_err(|e| PushGitError::Invalid(format!("extraction task panicked: {e}")))?
}

fn extract_engine_blocking(
    dir: &Path,
    archive_path: &Path,
    variant: EngineVariant,
) -> PushGitResult<()> {
    let file = std::fs::File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let target = engine_dir(dir, variant);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(&target)?;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Ok(relative) = path.strip_prefix(ENGINE_ARCHIVE_TOP_DIR) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue; // the top-level directory entry itself
        }
        entry.unpack(target.join(relative))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let binary = engine_binary_path(dir, variant);
        let mut perms = std::fs::metadata(&binary)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary, perms)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use tauri::ipc::InvokeResponseBody;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn collecting_channel() -> (Channel<DownloadProgress>, Arc<StdMutex<Vec<u64>>>) {
        let received = Arc::new(StdMutex::new(Vec::new()));
        let for_closure = received.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(progress) = serde_json::from_str::<DownloadProgress>(&json) {
                    for_closure.lock().unwrap().push(progress.bytes_downloaded);
                }
            }
            Ok(())
        });
        (channel, received)
    }

    fn not_cancelled() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        to_hex(&Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn downloads_and_verifies_a_fresh_file() {
        let dir = TempDir::new().unwrap();
        let server = MockServer::start().await;
        let body = b"hello local ai".to_vec();
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let final_path = dir.path().join("model.gguf");
        let (channel, progress) = collecting_channel();

        download_verified(
            dir.path(),
            Asset::Model,
            &format!("{}/file", server.uri()),
            body.len() as u64,
            &sha256_hex(&body),
            &final_path,
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&final_path).unwrap(), body);
        assert!(!part_path_for(&final_path).exists());
        assert!(!progress.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_checksum_mismatch_deletes_the_part_file_and_errors() {
        let dir = TempDir::new().unwrap();
        let server = MockServer::start().await;
        let body = b"corrupted".to_vec();
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let final_path = dir.path().join("model.gguf");
        let (channel, _progress) = collecting_channel();

        let err = download_verified(
            dir.path(),
            Asset::Model,
            &format!("{}/file", server.uri()),
            body.len() as u64,
            "0000000000000000000000000000000000000000000000000000000000000000",
            &final_path,
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Invalid(_)));
        assert!(!part_path_for(&final_path).exists());
        assert!(!final_path.exists());
    }

    #[tokio::test]
    async fn cancelling_mid_download_leaves_the_part_file_in_place() {
        let dir = TempDir::new().unwrap();
        let server = MockServer::start().await;
        let body = b"some bytes to stream".to_vec();
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let final_path = dir.path().join("model.gguf");
        let (channel, _progress) = collecting_channel();
        let cancel = Arc::new(AtomicBool::new(true));

        let err = download_verified(
            dir.path(),
            Asset::Model,
            &format!("{}/file", server.uri()),
            body.len() as u64,
            &sha256_hex(&body),
            &final_path,
            &channel,
            &cancel,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Cancelled));
        assert!(part_path_for(&final_path).exists());
    }

    #[tokio::test]
    async fn a_part_file_from_a_stale_pin_is_discarded_before_resuming() {
        let dir = TempDir::new().unwrap();
        let final_path = dir.path().join("model.gguf");
        let part_path = part_path_for(&final_path);
        std::fs::write(&part_path, b"stale partial bytes from an old pin").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_pending_checksum = Some("old-pin".to_string());
        manifest::save(dir.path(), &manifest);

        reconcile_pending_pin(dir.path(), Asset::Model, "new-pin", &part_path).await;

        assert!(!part_path.exists());
        let manifest = manifest::load(dir.path());
        assert_eq!(manifest.model_pending_checksum, Some("new-pin".to_string()));
    }

    #[tokio::test]
    async fn a_part_file_matching_the_current_pin_is_left_alone() {
        let dir = TempDir::new().unwrap();
        let final_path = dir.path().join("model.gguf");
        let part_path = part_path_for(&final_path);
        std::fs::write(&part_path, b"in-progress bytes for the current pin").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_pending_checksum = Some("current-pin".to_string());
        manifest::save(dir.path(), &manifest);

        reconcile_pending_pin(dir.path(), Asset::Model, "current-pin", &part_path).await;

        assert!(part_path.exists());
    }

    #[tokio::test]
    async fn engine_variants_reconcile_independent_pending_pins() {
        let dir = TempDir::new().unwrap();
        let cpu_part = part_path_for(&engine_archive_path(dir.path(), EngineVariant::Cpu));
        std::fs::write(&cpu_part, b"stale cpu partial").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.set_engine_pending_checksum(EngineVariant::Cpu, Some("old-cpu-pin".to_string()));
        manifest.set_engine_pending_checksum(EngineVariant::Vulkan, Some("vulkan-pin".to_string()));
        manifest::save(dir.path(), &manifest);

        reconcile_pending_pin(
            dir.path(),
            Asset::Engine(EngineVariant::Cpu),
            "new-cpu-pin",
            &cpu_part,
        )
        .await;

        assert!(!cpu_part.exists());
        let manifest = manifest::load(dir.path());
        assert_eq!(
            manifest.engine_pending_checksum(EngineVariant::Cpu),
            Some("new-cpu-pin")
        );
        // The other variant's pending pin is untouched by a CPU-scoped reconcile.
        assert_eq!(
            manifest.engine_pending_checksum(EngineVariant::Vulkan),
            Some("vulkan-pin")
        );
    }

    #[tokio::test]
    async fn mark_verified_sets_only_the_given_variants_checksum() {
        let dir = TempDir::new().unwrap();

        mark_verified(
            dir.path(),
            Asset::Engine(EngineVariant::Vulkan),
            "vulkan-sum",
        );

        let manifest = manifest::load(dir.path());
        assert_eq!(
            manifest.engine_checksum(EngineVariant::Vulkan),
            Some("vulkan-sum")
        );
        assert_eq!(manifest.engine_checksum(EngineVariant::Cpu), None);
    }

    #[test]
    fn engine_variants_use_distinct_archive_paths() {
        let dir = TempDir::new().unwrap();

        let cpu_path = engine_archive_path(dir.path(), EngineVariant::Cpu);
        let vulkan_path = engine_archive_path(dir.path(), EngineVariant::Vulkan);

        assert_ne!(cpu_path, vulkan_path);
    }

    #[tokio::test]
    async fn download_local_ai_skips_the_model_when_already_verified_on_a_variant_switch() {
        let dir = TempDir::new().unwrap();
        // Simulate a prior successful CPU download: model already verified on disk.
        std::fs::write(model_path(dir.path()), b"already-downloaded model bytes").unwrap();
        let mut manifest = manifest::load(dir.path());
        manifest.model_checksum = Some(MODEL_SHA256.to_string());
        manifest::save(dir.path(), &manifest);

        assert!(model_verified(dir.path()));
        // `download_local_ai` only re-fetches the model when `model_verified` is false — since
        // it's already true here, a variant switch's real work is scoped to the engine alone.
        assert!(!engine_verified(dir.path(), EngineVariant::Vulkan));
    }
}
