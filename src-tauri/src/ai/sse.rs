// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hand-rolled Server-Sent-Events parsing, shared by the OpenAI-compatible and Anthropic
//! adapters — `reqwest` has no built-in SSE support, and the wire format (buffer to the next
//! blank-line boundary, strip each line's `field: ` prefix) is simple enough not to justify a
//! dependency. No reconnect logic: this is a single request/response per "Generate with AI"
//! click, not a long-lived subscription.

use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream::{Stream, StreamExt};

use crate::error::{PushGitError, PushGitResult};

/// No data for this long (not overall request duration — a generation can legitimately take
/// a while) maps to a cancellable/error path rather than a silent hang.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// One parsed SSE event: an optional `event:` line (Anthropic types its events;
/// OpenAI-compatible servers don't send this field at all, leaving it `None`) plus the
/// concatenated `data:` line(s), prefix stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Buffers a streamed HTTP response up to each blank-line event boundary and hands back one
/// parsed [`SseEvent`] at a time.
pub struct SseReader {
    buf: Vec<u8>,
    stream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
}

impl SseReader {
    pub fn new(response: reqwest::Response) -> Self {
        Self {
            buf: Vec::new(),
            stream: Box::pin(response.bytes_stream()),
        }
    }

    /// The next event, or `None` once the stream has ended cleanly with no more data
    /// buffered.
    pub async fn next_event(&mut self) -> PushGitResult<Option<SseEvent>> {
        loop {
            if let Some((pos, delim_len)) = find_block_boundary(&self.buf) {
                let block: Vec<u8> = self.buf.drain(..pos).collect();
                self.buf.drain(..delim_len);
                if block.iter().all(u8::is_ascii_whitespace) {
                    continue; // a blank keep-alive block — nothing to parse
                }
                return Ok(Some(parse_event(&block)));
            }

            match self.stream.next().await {
                Some(Ok(bytes)) => self.buf.extend_from_slice(&bytes),
                Some(Err(e)) => {
                    return Err(PushGitError::Invalid(format!("AI request failed: {e}")))
                }
                None => {
                    if self.buf.iter().any(|b| !b.is_ascii_whitespace()) {
                        let block = std::mem::take(&mut self.buf);
                        return Ok(Some(parse_event(&block)));
                    }
                    return Ok(None);
                }
            }
        }
    }
}

/// Checks an HTTP response's status before handing it to [`SseReader`], surfacing the
/// provider's error body where available and calling out a bad API key specifically.
pub async fn check_response_status(
    response: reqwest::Response,
) -> PushGitResult<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = if body.trim().is_empty() {
        status.to_string()
    } else {
        format!("{status}: {}", body.trim())
    };
    if status.as_u16() == 401 || status.as_u16() == 403 {
        Err(PushGitError::Invalid(format!(
            "AI provider rejected the API key ({detail})"
        )))
    } else {
        Err(PushGitError::Invalid(format!(
            "AI provider error ({detail})"
        )))
    }
}

fn parse_event(block: &[u8]) -> SseEvent {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim_start().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start());
        }
        // `id:`, retry directives, and `:`-prefixed comments carry nothing either adapter
        // needs, so they're silently dropped.
    }
    SseEvent {
        event,
        data: data_lines.join("\n"),
    }
}

/// The earliest blank-line boundary (`\n\n` or `\r\n\r\n`) in `buf`, and how many bytes that
/// delimiter itself occupies — real SSE servers overwhelmingly use bare `\n\n`, but a
/// `\r\n\r\n` terminator doesn't contain `\n\n` as a substring, so both are checked rather
/// than assuming one.
fn find_block_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = find_subslice(buf, b"\n\n").map(|pos| (pos, 2));
    let crlf = find_subslice(buf, b"\r\n\r\n").map(|pos| (pos, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    fn reader_for(chunks: Vec<&'static str>) -> SseReader {
        let items: Vec<reqwest::Result<Bytes>> = chunks
            .into_iter()
            .map(|c| Ok(Bytes::from_static(c.as_bytes())))
            .collect();
        SseReader {
            buf: Vec::new(),
            stream: Box::pin(stream::iter(items)),
        }
    }

    #[tokio::test]
    async fn parses_a_flat_openai_style_data_only_event() {
        let mut reader = reader_for(vec!["data: {\"choices\":[]}\n\n"]);

        let event = reader.next_event().await.unwrap().unwrap();

        assert_eq!(event.event, None);
        assert_eq!(event.data, "{\"choices\":[]}");
    }

    #[tokio::test]
    async fn parses_a_typed_anthropic_style_event() {
        let mut reader = reader_for(vec!["event: content_block_delta\ndata: {\"delta\":{}}\n\n"]);

        let event = reader.next_event().await.unwrap().unwrap();

        assert_eq!(event.event.as_deref(), Some("content_block_delta"));
        assert_eq!(event.data, "{\"delta\":{}}");
    }

    #[tokio::test]
    async fn reassembles_an_event_split_across_multiple_chunks() {
        let mut reader = reader_for(vec!["data: {\"a\":", "1}\n", "\n"]);

        let event = reader.next_event().await.unwrap().unwrap();

        assert_eq!(event.data, "{\"a\":1}");
    }

    #[tokio::test]
    async fn returns_none_once_the_stream_ends_with_no_buffered_data() {
        let mut reader = reader_for(vec!["data: one\n\n"]);

        assert!(reader.next_event().await.unwrap().is_some());
        assert_eq!(reader.next_event().await.unwrap(), None);
    }

    #[tokio::test]
    async fn parses_a_final_event_with_no_trailing_blank_line() {
        let mut reader = reader_for(vec!["data: last"]);

        let event = reader.next_event().await.unwrap().unwrap();

        assert_eq!(event.data, "last");
        assert_eq!(reader.next_event().await.unwrap(), None);
    }

    #[tokio::test]
    async fn skips_a_blank_keep_alive_block() {
        let mut reader = reader_for(vec!["\n\n", "data: real\n\n"]);

        let event = reader.next_event().await.unwrap().unwrap();

        assert_eq!(event.data, "real");
    }
}
