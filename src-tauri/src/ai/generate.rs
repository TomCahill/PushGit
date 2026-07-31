// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Orchestrates one "Generate with AI" click: reads settings + the keyring, builds the
//! prompt from the staged diff, and dispatches to the transport-specific adapter.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;

use crate::config;
use crate::diff;
use crate::error::{PushGitError, PushGitResult};
use crate::repo;

use super::{anthropic, openai, prompt, AiTransport};

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

    let full_prompt = prompt::build_prompt(&staged, &settings.instructions);
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
    }
}
