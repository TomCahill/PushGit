// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs the downloaded `llama-server` binary as a long-lived subprocess and hands the result
//! off to the existing `ai::openai` adapter — llama-server speaks the same OpenAI-compatible
//! chat-completions wire format (confirmed directly against a running instance), so this
//! module's only job is acquiring a ready, addressable server, not a new HTTP/generation path.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, Command};
use tokio::time::Instant;

use crate::error::{PushGitError, PushGitResult};

use super::{engine_binary_path, model_path, resolve_dir};

/// 16384, not the originally-planned 4096 — real usage hit `llama-server`'s
/// `exceed_context_size_error` on an ordinary multi-file diff: the existing 32KB diff-
/// truncation cap (`ai::prompt::MAX_DIFF_BYTES`, sized for cloud models with much larger
/// context windows) can alone tokenize past 6000+ tokens, before the system prompt and
/// generation budget are even added. Qwen2.5-Coder-1.5B natively supports up to 32768 tokens
/// with no RoPE/YaRN scaling needed, so 16384 gives large headroom over that worst case for a
/// modest KV-cache RAM increase, without having to fork the diff-truncation cap per transport.
const CTX_SIZE: &str = "16384";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub struct LocalEngineHandle {
    child: Child,
    port: u16,
}

impl LocalEngineHandle {
    /// The base URL the existing `ai::openai::stream()` adapter should be pointed at —
    /// `{base_url}/chat/completions`, matching every other OpenAI-compatible transport.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Best-effort, synchronous — used from the app-exit `RunEvent` handler, which isn't async.
    /// `kill_on_drop(true)` on the underlying `Child` is a backup for any path that drops this
    /// handle without calling this explicitly.
    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Spawns `llama-server` if nothing is running yet (or the previous instance has since exited),
/// waits for it to report healthy, and returns its base URL — otherwise returns the existing,
/// still-running instance's base URL immediately. `cancel` is checked throughout startup, not
/// only during the generation stream that follows: otherwise clicking "stop" while the engine
/// is still booting would do nothing until `STARTUP_TIMEOUT` elapsed on its own.
pub async fn ensure_local_engine_running(
    handle: &mut Option<LocalEngineHandle>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<String> {
    if let Some(existing) = handle {
        if existing.is_running() {
            return Ok(existing.base_url());
        }
    }

    let dir = resolve_dir()?;
    let new_handle = spawn_and_wait_healthy(&dir, cancel).await?;
    let base_url = new_handle.base_url();
    *handle = Some(new_handle);
    Ok(base_url)
}

async fn spawn_and_wait_healthy(
    dir: &std::path::Path,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<LocalEngineHandle> {
    let mut child = Command::new(engine_binary_path(dir))
        .arg("--model")
        .arg(model_path(dir))
        .arg("--ctx-size")
        .arg(CTX_SIZE)
        .arg("--port")
        .arg("0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| PushGitError::Invalid(format!("failed to start the local AI engine: {e}")))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PushGitError::Invalid("local AI engine had no stderr".to_string()))?;
    let mut lines = BufReader::new(stderr).lines();

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let port = match read_assigned_port(&mut lines, cancel, deadline).await {
        Ok(port) => port,
        Err(e) => {
            let _ = child.start_kill();
            return Err(e);
        }
    };

    // Keep draining stderr for the rest of the process's life — llama-server logs per-request
    // timing lines, and an unread pipe would eventually fill and block it from writing further.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

    let health_url = format!("http://127.0.0.1:{port}/health");
    if let Err(e) = wait_until_healthy(&health_url, cancel, deadline).await {
        let _ = child.start_kill();
        return Err(e);
    }

    Ok(LocalEngineHandle { child, port })
}

/// Generic over the line source (rather than tied to `ChildStderr`) so it's testable against an
/// in-memory stream without spawning a real subprocess.
async fn read_assigned_port<R: AsyncBufRead + Unpin>(
    lines: &mut Lines<R>,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> PushGitResult<u16> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(PushGitError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PushGitError::Invalid(
                "local AI engine did not start in time".to_string(),
            ));
        }

        match tokio::time::timeout(remaining.min(CANCEL_POLL_INTERVAL), lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if let Some(port) = parse_listening_port(&line) {
                    return Ok(port);
                }
            }
            Ok(Ok(None)) => {
                return Err(PushGitError::Invalid(
                    "local AI engine exited before it started listening".to_string(),
                ))
            }
            Ok(Err(e)) => {
                return Err(PushGitError::Invalid(format!(
                    "failed to read local AI engine output: {e}"
                )))
            }
            Err(_elapsed) => continue, // no line within this slice — loop, re-check cancel/deadline
        }
    }
}

/// Matches llama-server's actual startup line, confirmed against a running instance:
/// `<ts> I srv  llama_server: listening on http://127.0.0.1:<port>`, written to stderr.
fn parse_listening_port(line: &str) -> Option<u16> {
    const MARKER: &str = "listening on http://";
    let after = &line[line.find(MARKER)? + MARKER.len()..];
    after.rsplit(':').next()?.trim().parse().ok()
}

async fn wait_until_healthy(
    health_url: &str,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> PushGitResult<()> {
    let client = reqwest::Client::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(PushGitError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(PushGitError::Invalid(
                "local AI engine did not become healthy in time".to_string(),
            ));
        }
        if let Ok(response) = client.get(health_url).send().await {
            if response.status().is_success() {
                return Ok(());
            }
        }
        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    fn not_cancelled() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn parses_the_port_out_of_a_real_startup_line() {
        let line = "0.00.025.679 I srv  llama_server: listening on http://127.0.0.1:42443";

        assert_eq!(parse_listening_port(line), Some(42443));
    }

    #[test]
    fn returns_none_for_an_unrelated_line() {
        let line = "0.00.011.614 I srv    load_model: loading model '../../tiny.gguf'";

        assert_eq!(parse_listening_port(line), None);
    }

    #[tokio::test]
    async fn finds_the_port_once_the_listening_line_arrives() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();
        writer
            .write_all(b"some banner line\nlistening on http://127.0.0.1:9999\n")
            .await
            .unwrap();

        let port = read_assigned_port(
            &mut lines,
            &not_cancelled(),
            Instant::now() + STARTUP_TIMEOUT,
        )
        .await
        .unwrap();

        assert_eq!(port, 9999);
    }

    #[tokio::test]
    async fn times_out_if_no_matching_line_arrives_before_the_deadline() {
        let (_writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();

        let err = read_assigned_port(
            &mut lines,
            &not_cancelled(),
            Instant::now() + Duration::from_millis(50),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Invalid(_)));
    }

    #[tokio::test]
    async fn stops_immediately_when_already_cancelled() {
        let (_writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();
        let cancel = Arc::new(AtomicBool::new(true));

        let err = read_assigned_port(&mut lines, &cancel, Instant::now() + STARTUP_TIMEOUT)
            .await
            .unwrap_err();

        assert!(matches!(err, PushGitError::Cancelled));
    }

    #[tokio::test]
    async fn errors_if_the_stream_ends_before_a_listening_line_arrives() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();
        writer
            .write_all(b"engine crashed on startup\n")
            .await
            .unwrap();
        drop(writer);

        let err = read_assigned_port(
            &mut lines,
            &not_cancelled(),
            Instant::now() + STARTUP_TIMEOUT,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Invalid(_)));
    }

    #[tokio::test]
    async fn wait_until_healthy_times_out_when_the_endpoint_never_succeeds() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/health"))
            .respond_with(wiremock::ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let err = wait_until_healthy(
            &format!("{}/health", server.uri()),
            &not_cancelled(),
            Instant::now() + Duration::from_millis(300),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Invalid(_)));
    }

    #[tokio::test]
    async fn wait_until_healthy_succeeds_once_the_endpoint_returns_success() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/health"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        wait_until_healthy(
            &format!("{}/health", server.uri()),
            &not_cancelled(),
            Instant::now() + STARTUP_TIMEOUT,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn wait_until_healthy_stops_immediately_when_already_cancelled() {
        let cancel = Arc::new(AtomicBool::new(true));

        let err = wait_until_healthy(
            "http://127.0.0.1:1/health",
            &cancel,
            Instant::now() + STARTUP_TIMEOUT,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Cancelled));
    }
}
