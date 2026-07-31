// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The native Anthropic Messages API adapter — not OpenAI-wire-compatible, so it gets its
//! own adapter rather than reusing `openai`'s. Two structural differences beyond URL/headers:
//! the system prompt is a top-level `system` field (not a message in the `messages` array),
//! and streaming events are typed (`event: content_block_delta`/`message_stop`) with delta
//! text nested at `delta.text`, rather than OpenAI's flat `data:`-only chunks. The shared
//! `sse` helper handles the common `data:`-line extraction either way; this file is where
//! those differences actually live.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;

use crate::error::{PushGitError, PushGitResult};

use super::generate::AiChunk;
use super::sse::{check_response_status, SseReader, IDLE_TIMEOUT};

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// A hard API requirement (the Messages API rejects requests without it), not a value worth
/// exposing in Settings — generous for a single-line commit subject, not user-tunable.
const MAX_TOKENS: u32 = 1024;

#[derive(Serialize)]
struct UserMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    system: &'a str,
    max_tokens: u32,
    stream: bool,
    messages: Vec<UserMessage<'a>>,
}

pub async fn stream(
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    prompt: &str,
    channel: &Channel<AiChunk>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = MessagesRequest {
        model,
        system: prompt,
        max_tokens: MAX_TOKENS,
        stream: true,
        // The Messages API requires a non-empty `messages` array starting with a `user`
        // turn even though the actual instructions live in `system` above — this fixed
        // placeholder just triggers the assistant's reply.
        messages: vec![UserMessage {
            role: "user",
            content: "Generate the commit message now.",
        }],
    };

    let mut request = client
        .post(&url)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        request = request.header("x-api-key", key);
    }

    let response = request
        .send()
        .await
        .map_err(|e| PushGitError::Invalid(format!("AI request failed: {e}")))?;
    let response = check_response_status(response).await?;
    let mut reader = SseReader::new(response);

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(PushGitError::Cancelled);
        }

        let event = match tokio::time::timeout(IDLE_TIMEOUT, reader.next_event()).await {
            Ok(result) => result?,
            Err(_) => return Err(PushGitError::Invalid("AI request timed out".to_string())),
        };
        let Some(event) = event else {
            break;
        };

        match event.event.as_deref() {
            Some("message_stop") => break,
            Some("content_block_delta") => {
                let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
                    continue;
                };
                let text = value
                    .get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(Value::as_str);
                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    let _ = channel.send(AiChunk {
                        text: text.to_string(),
                    });
                }
            }
            _ => {} // message_start/content_block_start/ping/etc. carry nothing we need
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tauri::ipc::InvokeResponseBody;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn collecting_channel() -> (Channel<AiChunk>, Arc<Mutex<Vec<String>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        let for_closure = received.clone();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(chunk) = serde_json::from_str::<AiChunk>(&json) {
                    for_closure.lock().unwrap().push(chunk.text);
                }
            }
            Ok(())
        });
        (channel, received)
    }

    fn not_cancelled() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[tokio::test]
    async fn assembles_a_stream_from_typed_content_block_delta_events() {
        let server = MockServer::start().await;
        let body = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"text\":\"fix: \"}}\n\n",
            "event: content_block_delta\ndata: {\"delta\":{\"text\":\"bug\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("anthropic-version", ANTHROPIC_VERSION))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let (channel, received) = collecting_channel();
        stream(
            &server.uri(),
            "claude-sonnet-5",
            Some("secret-key"),
            "prompt",
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap();

        assert_eq!(*received.lock().unwrap(), vec!["fix: ", "bug"]);
    }

    #[tokio::test]
    async fn a_403_response_surfaces_as_a_bad_api_key_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;

        let (channel, _received) = collecting_channel();
        let err = stream(
            &server.uri(),
            "claude-sonnet-5",
            Some("bad-key"),
            "prompt",
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("API key"));
    }

    #[tokio::test]
    async fn an_already_cancelled_token_stops_the_stream_without_emitting_chunks() {
        let server = MockServer::start().await;
        let body = "event: content_block_delta\ndata: {\"delta\":{\"text\":\"too late\"}}\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let (channel, received) = collecting_channel();
        let cancel = Arc::new(AtomicBool::new(true));
        let err = stream(
            &server.uri(),
            "claude-sonnet-5",
            None,
            "prompt",
            &channel,
            &cancel,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, PushGitError::Cancelled));
        assert!(received.lock().unwrap().is_empty());
    }
}
