// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Base64-wrapping raw file/blob bytes for the frontend's binary/image diff preview — see
//! `.private/feature/image-binary-diff/PLAN.md`. Separate from `model`'s diff-computation
//! types since this has nothing to do with hunks; it exists purely to get bytes across the
//! JSON-based Tauri IPC boundary.

use base64::Engine;
use serde::Serialize;

/// A single side's oversize cutoff. Generous enough to cover any real image asset; beyond
/// this, base64-inflating and shipping the bytes over IPC isn't worth it for a preview.
const MAX_PREVIEW_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum BinaryPreview {
    Content { base64: String, byte_len: u64 },
    TooLarge { byte_len: u64 },
}

/// Wraps raw bytes for one side of a binary diff. `bytes` being empty is not treated
/// specially here — the caller (frontend) already knows from `FileDiff::status`/the
/// conflict's own ours/theirs shape whether a side is genuinely absent, so this doesn't need
/// its own empty-vs-absent sentinel the way `external_tools::resolve_side_bytes` does.
pub fn preview_from_bytes(bytes: Vec<u8>) -> BinaryPreview {
    let byte_len = bytes.len() as u64;
    if bytes.len() > MAX_PREVIEW_BYTES {
        BinaryPreview::TooLarge { byte_len }
    } else {
        BinaryPreview::Content {
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            byte_len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_from_bytes_encodes_content_under_the_cap() {
        let preview = preview_from_bytes(b"hello".to_vec());
        match preview {
            BinaryPreview::Content { base64, byte_len } => {
                assert_eq!(byte_len, 5);
                assert_eq!(
                    base64::engine::general_purpose::STANDARD
                        .decode(base64)
                        .unwrap(),
                    b"hello"
                );
            }
            BinaryPreview::TooLarge { .. } => panic!("expected Content"),
        }
    }

    #[test]
    fn preview_from_bytes_reports_too_large_over_the_cap() {
        let bytes = vec![0u8; MAX_PREVIEW_BYTES + 1];
        let preview = preview_from_bytes(bytes);
        match preview {
            BinaryPreview::TooLarge { byte_len } => {
                assert_eq!(byte_len, (MAX_PREVIEW_BYTES + 1) as u64)
            }
            BinaryPreview::Content { .. } => panic!("expected TooLarge"),
        }
    }
}
