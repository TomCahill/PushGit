// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Runs the downloaded `llama-server` binary as a long-lived subprocess and hands the result
//! off to the existing `ai::openai` adapter — llama-server speaks the same OpenAI-compatible
//! chat-completions wire format (confirmed directly against a running instance), so this
//! module's only job is acquiring a ready, addressable server, not a new HTTP/generation path.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, Command};
use tokio::time::Instant;

use crate::error::{PushGitError, PushGitResult};

use super::{engine_binary_path, model_path, resolve_dir, EngineVariant};

/// How many of the most recent pre-listening stderr lines to keep around for diagnostics — a
/// GPU driver failure (Phase D) is the first realistic case where the generic "exited before it
/// started listening" message needs to carry an actual reason, so this captures just enough
/// context to be useful without holding the engine's entire startup log in memory.
const RECENT_STDERR_LINES: usize = 5;

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
    variant: EngineVariant,
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

/// Spawns `llama-server` if nothing is running yet, the previous instance has since exited, or
/// it's running the wrong `EngineVariant` for the currently-saved setting — waits for it to
/// report healthy, and returns its base URL. Otherwise returns the existing, already-running,
/// already-matching instance's base URL immediately. `cancel` is checked throughout startup,
/// not only during the generation stream that follows: otherwise clicking "stop" while the
/// engine is still booting would do nothing until `STARTUP_TIMEOUT` elapsed on its own.
pub async fn ensure_local_engine_running(
    handle: &mut Option<LocalEngineHandle>,
    variant: EngineVariant,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<String> {
    if !needs_fresh_spawn(handle, variant) {
        return Ok(handle.as_ref().expect("just checked Some").base_url());
    }

    let dir = resolve_dir()?;
    let new_handle = spawn_and_wait_healthy(&dir, variant, cancel).await?;
    let base_url = new_handle.base_url();
    *handle = Some(new_handle);
    Ok(base_url)
}

/// Whether `handle` needs replacing with a fresh spawn for `variant` — `None`, a dead process,
/// or a live process running the wrong `EngineVariant` (a setting change since this handle was
/// spawned) all need one; only a live, matching-variant process doesn't. A mismatched-but-still-
/// running handle is killed and cleared as a side effect, extending the pre-Phase-D "handle is
/// `None` or the child has exited" restart condition to "...or is the wrong variant," rather
/// than a separate code path. Factored out from `ensure_local_engine_running` so this decision
/// is testable without needing a real `llama-server` binary to spawn.
fn needs_fresh_spawn(handle: &mut Option<LocalEngineHandle>, variant: EngineVariant) -> bool {
    let Some(existing) = handle else {
        return true;
    };
    if existing.variant == variant && existing.is_running() {
        return false;
    }
    if existing.is_running() {
        existing.kill();
    }
    *handle = None;
    true
}

async fn spawn_and_wait_healthy(
    dir: &std::path::Path,
    variant: EngineVariant,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<LocalEngineHandle> {
    let mut child = Command::new(engine_binary_path(dir, variant))
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

    Ok(LocalEngineHandle {
        child,
        port,
        variant,
    })
}

/// Generic over the line source (rather than tied to `ChildStderr`) so it's testable against an
/// in-memory stream without spawning a real subprocess. Keeps the last `RECENT_STDERR_LINES`
/// non-matching lines around so a startup failure (e.g. the Vulkan backend can't initialize —
/// missing driver, incompatible GPU) surfaces an actual reason instead of a generic message.
async fn read_assigned_port<R: AsyncBufRead + Unpin>(
    lines: &mut Lines<R>,
    cancel: &Arc<AtomicBool>,
    deadline: Instant,
) -> PushGitResult<u16> {
    let mut recent: VecDeque<String> = VecDeque::with_capacity(RECENT_STDERR_LINES);
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
                if recent.len() == RECENT_STDERR_LINES {
                    recent.pop_front();
                }
                recent.push_back(line);
            }
            Ok(Ok(None)) => {
                return Err(PushGitError::Invalid(format!(
                    "local AI engine exited before it started listening{}",
                    recent_lines_suffix(&recent)
                )))
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

fn recent_lines_suffix(recent: &VecDeque<String>) -> String {
    if recent.is_empty() {
        String::new()
    } else {
        format!(
            " — last output: {}",
            recent.iter().cloned().collect::<Vec<_>>().join(" | ")
        )
    }
}

/// Runs `llama-server --list-devices` for `variant` and returns the first non-`(none)` device
/// name reported, or `None` if no GPU device is available — confirmed directly against the
/// pinned binary: no `--model` needed, runs and exits almost instantly, output on stdout. Only
/// meaningful for the Vulkan variant (the CPU build has no Vulkan backend compiled in and
/// always reports `(none)`); callers gate on that themselves.
pub async fn detect_gpu_device(dir: &std::path::Path, variant: EngineVariant) -> Option<String> {
    let output = Command::new(engine_binary_path(dir, variant))
        .arg("--list-devices")
        .output()
        .await
        .ok()?;
    parse_device_list(&String::from_utf8_lossy(&output.stdout))
}

/// Parses `llama-server --list-devices`' stdout: either `Available devices:\n  (none)` or
/// `Available devices:\n  VulkanN: <device name> (...)`, one device per line.
fn parse_device_list(output: &str) -> Option<String> {
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "Available devices:" {
            continue;
        }
        let device_line = lines.next()?.trim();
        if device_line == "(none)" {
            return None;
        }
        let (_, after_colon) = device_line.split_once(':')?;
        let name = after_colon.split(" (").next().unwrap_or(after_colon).trim();
        return Some(name.to_string());
    }
    None
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
    async fn a_startup_failure_error_includes_the_recent_stderr_lines() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        let mut lines = BufReader::new(reader).lines();
        writer
            .write_all(b"loading Vulkan backend\nvkCreateInstance failed: driver not found\n")
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

        let PushGitError::Invalid(message) = err else {
            panic!("expected PushGitError::Invalid");
        };
        assert!(message.contains("loading Vulkan backend"));
        assert!(message.contains("vkCreateInstance failed: driver not found"));
    }

    #[test]
    fn parse_device_list_returns_none_for_the_cpu_only_build() {
        let output = "Available devices:\n  (none)\n";

        assert_eq!(parse_device_list(output), None);
    }

    #[test]
    fn parse_device_list_returns_the_named_device_for_a_vulkan_build() {
        let output =
            "Available devices:\n  Vulkan0: NVIDIA GeForce RTX 3060 (12005 MiB, 12005 MiB free)\n";

        assert_eq!(
            parse_device_list(output),
            Some("NVIDIA GeForce RTX 3060".to_string())
        );
    }

    #[test]
    fn parse_device_list_returns_none_when_the_marker_line_is_absent() {
        let output = "some unrelated output\n";

        assert_eq!(parse_device_list(output), None);
    }

    fn spawn_dummy_process() -> Child {
        Command::new("sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn dummy `sleep` process for a test double")
    }

    #[test]
    fn needs_fresh_spawn_is_true_when_no_handle_exists() {
        let mut handle: Option<LocalEngineHandle> = None;

        assert!(needs_fresh_spawn(&mut handle, EngineVariant::Cpu));
    }

    #[tokio::test]
    async fn needs_fresh_spawn_is_false_for_a_running_handle_of_the_matching_variant() {
        let mut handle = Some(LocalEngineHandle {
            child: spawn_dummy_process(),
            port: 12345,
            variant: EngineVariant::Cpu,
        });

        assert!(!needs_fresh_spawn(&mut handle, EngineVariant::Cpu));
        assert!(
            handle.is_some(),
            "a matching, running handle is left in place"
        );
    }

    #[tokio::test]
    async fn needs_fresh_spawn_kills_and_clears_a_running_handle_of_the_wrong_variant() {
        let mut handle = Some(LocalEngineHandle {
            child: spawn_dummy_process(),
            port: 12345,
            variant: EngineVariant::Cpu,
        });

        let result = needs_fresh_spawn(&mut handle, EngineVariant::Vulkan);

        assert!(result);
        assert!(handle.is_none(), "the mismatched handle is cleared");
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
