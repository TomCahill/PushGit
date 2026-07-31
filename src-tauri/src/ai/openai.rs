// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The OpenAI-compatible chat-completions adapter — covers local servers (Ollama,
//! llama.cpp's server, LM Studio) and most cloud providers, since they all speak this same
//! wire format.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;

use crate::error::{PushGitError, PushGitResult};

use super::generate::AiChunk;
use super::sse::{check_response_status, SseReader, IDLE_TIMEOUT};

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
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
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = ChatRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: prompt,
            },
            ChatMessage {
                role: "user",
                content: "Generate the commit message now.",
            },
        ],
        stream: true,
    };

    let mut request = client.post(&url).json(&body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        request = request.bearer_auth(key);
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
        if event.data == "[DONE]" {
            break;
        }

        let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
            continue; // a malformed/keep-alive chunk — skip rather than fail the whole stream
        };
        let delta = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(Value::as_str);
        if let Some(text) = delta.filter(|t| !t.is_empty()) {
            let _ = channel.send(AiChunk {
                text: text.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tauri::ipc::InvokeResponseBody;
    use wiremock::matchers::{method, path};
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
    async fn assembles_a_full_stream_from_multiple_delta_chunks() {
        let server = MockServer::start().await;
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"feat: \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"add thing\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let (channel, received) = collecting_channel();
        stream(
            &server.uri(),
            "llama3.1",
            None,
            "prompt",
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap();

        assert_eq!(*received.lock().unwrap(), vec!["feat: ", "add thing"]);
    }

    #[tokio::test]
    async fn a_401_response_surfaces_as_a_bad_api_key_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid api key"))
            .mount(&server)
            .await;

        let (channel, _received) = collecting_channel();
        let err = stream(
            &server.uri(),
            "llama3.1",
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
    async fn a_malformed_chunk_is_skipped_without_failing_the_stream() {
        let server = MockServer::start().await;
        let body = concat!(
            "data: not json at all\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let (channel, received) = collecting_channel();
        stream(
            &server.uri(),
            "llama3.1",
            None,
            "prompt",
            &channel,
            &not_cancelled(),
        )
        .await
        .unwrap();

        assert_eq!(*received.lock().unwrap(), vec!["ok"]);
    }

    #[tokio::test]
    async fn an_already_cancelled_token_stops_the_stream_without_emitting_chunks() {
        let server = MockServer::start().await;
        let body =
            "data: {\"choices\":[{\"delta\":{\"content\":\"too late\"}}]}\n\ndata: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let (channel, received) = collecting_channel();
        let cancel = Arc::new(AtomicBool::new(true));
        let err = stream(&server.uri(), "llama3.1", None, "prompt", &channel, &cancel)
            .await
            .unwrap_err();

        assert!(matches!(err, PushGitError::Cancelled));
        assert!(received.lock().unwrap().is_empty());
    }
}
