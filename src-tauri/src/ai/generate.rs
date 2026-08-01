// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Orchestrates one "Generate with AI" click: reads settings + the keyring, builds the
//! prompt from the staged diff, and dispatches to the transport-specific adapter.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;

use crate::config;
use crate::diff;
use crate::error::{PushGitError, PushGitResult};
use crate::repo;

use super::local::LocalEngineHandle;
use super::sse::IDLE_TIMEOUT;
use super::{anthropic, local, openai, prompt, AiTransport};

/// Fixed generation parameters for the managed local transport only — an API requirement-style
/// constant, not exposed in Settings, matching how the Anthropic adapter already hardcodes its
/// own `max_tokens`. Deterministic, factual output is what a commit-message summary needs, not
/// creative variation.
const LOCAL_TEMPERATURE: f32 = 0.1;
/// Headroom for a title plus a handful of short bullets, not a tight single-line budget —
/// title + up to `StagingPanel.svelte`'s `MAX_LOCAL_BULLETS` bullets fits comfortably under
/// this well before the frontend's own cap-and-cancel logic (below) would kick in first anyway.
const LOCAL_MAX_TOKENS: u32 = 200;
/// PushGit's own reinforcement, not user text — appended ahead of the user's custom
/// instructions in `prompt::build_prompt`, so it can't be overridden by them.
///
/// Deliberately does **not** ask the model to cap itself at N bullets — verified empirically
/// (a large real diff, run end-to-end against a live `llama-server`) that a 1.5B model doesn't
/// reliably do that: told "at most 3-4 bullets," it either ignored the limit and enumerated a
/// bullet per file until it hit `LOCAL_MAX_TOKENS` mid-word, or — worse, told more mechanically
/// to stop after exactly N — degenerated into repeating the identical line until the same cap,
/// every time tested. This directive only asks for the right *shape* (a subject line, then
/// optionally a few dash-prefixed bullets grouped by theme); the actual bullet-count limit is
/// enforced in `StagingPanel.svelte`'s `capBullets()`, which freezes the displayed text and
/// cancels generation once enough bullets have streamed in, regardless of what the model does.
/// That enforcement also happens to catch the repetition-loop failure mode as a side effect —
/// it cancels once a 5th bullet-shaped line starts, whether or not the first four were sensible.
const LOCAL_STYLE_DIRECTIVE: &str = "Write a one-line commit subject first. You may optionally follow it with a blank line and then a few short bullet points, each on its own line starting with \"- \", summarizing the key changes grouped by theme rather than one bullet per file. Do not wrap the output in a code block, and do not repeat the same line.";
/// llama-server always serves whichever model it loaded regardless of this field's value — it
/// only has to be present in the request body.
const LOCAL_MODEL_NAME: &str = "local";
/// `sse::IDLE_TIMEOUT` (30s) is tuned for a cloud API gone quiet mid-stream — that's already a
/// bad sign at 30s for a hosted model. A CPU-bound `llama-server` prefilling a multi-thousand-
/// token diff before it emits its first token can legitimately take much longer than that on
/// modest hardware, especially now that `ai::local::engine`'s context window covers diffs up to
/// `ai::prompt::MAX_DIFF_BYTES`. This only bounds "silently produced nothing for this long" —
/// the user's own Stop button, not this timeout, is the actual way to abandon a slow generation.
const LOCAL_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// A streamed delta — the backend has no concept of "title" vs "description," it just emits
/// the model's raw text as it arrives; the frontend re-runs its own message-splitting logic
/// against the accumulating buffer.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct AiChunk {
    pub text: String,
}

pub async fn generate_commit_message(
    repo_path: &Path,
    channel: &Channel<AiChunk>,
    cancel: &Arc<AtomicBool>,
    local_engine: &tokio::sync::Mutex<Option<LocalEngineHandle>>,
) -> PushGitResult<()> {
    let settings = config::load_app_config().ai;
    let transport = settings
        .transport
        .ok_or_else(|| PushGitError::Invalid("no AI provider is configured".to_string()))?;

    let staged = diff::diff_staged(&repo::open(repo_path)?)?;
    if staged.is_empty() {
        return Err(PushGitError::Invalid(
            "nothing is staged to summarize".to_string(),
        ));
    }

    let extra_directive =
        matches!(transport, AiTransport::ManagedLocal { .. }).then_some(LOCAL_STYLE_DIRECTIVE);
    let full_prompt = prompt::build_prompt(&staged, &settings.instructions, extra_directive);
    let api_key = super::get_api_key();

    match transport {
        AiTransport::OpenAiCompatible { base_url, model } => {
            openai::stream(
                &base_url,
                &model,
                api_key.as_deref(),
                &full_prompt,
                channel,
                cancel,
                None,
                None,
                IDLE_TIMEOUT,
            )
            .await
        }
        AiTransport::Anthropic { base_url, model } => {
            anthropic::stream(
                &base_url,
                &model,
                api_key.as_deref(),
                &full_prompt,
                channel,
                cancel,
            )
            .await
        }
        AiTransport::ManagedLocal { engine_variant } => {
            // Held only long enough to spawn/health-check the engine, then released before
            // streaming — llama-server handles its own request concurrency (n_slots), so a
            // second generation started while this one streams doesn't need to queue behind
            // this lock, only behind the (much shorter) "is it already running" check.
            let base_url = {
                let mut guard = local_engine.lock().await;
                local::ensure_local_engine_running(&mut guard, engine_variant, cancel).await?
            };
            openai::stream(
                &base_url,
                LOCAL_MODEL_NAME,
                None,
                &full_prompt,
                channel,
                cancel,
                Some(LOCAL_TEMPERATURE),
                Some(LOCAL_MAX_TOKENS),
                LOCAL_IDLE_TIMEOUT,
            )
            .await
        }
    }
}
