// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! AI-assisted commit message generation: provider configuration (stored in
//! `config::AppConfig`), API key storage (OS keyring, never `config.json`), and the HTTP
//! adapters that stream a generated message back to the frontend.

mod anthropic;
mod generate;
mod keys;
mod openai;
mod prompt;
mod sse;

pub use generate::{generate_commit_message, AiChunk};
pub use keys::{clear_api_key, get_api_key, has_api_key, store_api_key};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AiTransport {
    OpenAiCompatible { base_url: String, model: String },
    Anthropic { base_url: String, model: String },
}

/// `transport: None` is the out-of-the-box, feature-inert state. `instructions` and
/// `cloud_warning_acknowledged` are plain non-secret values, stored in `config.json` alongside
/// everything else in `AppConfig` — unlike the API key (`ai::keys`), which never touches that
/// file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    #[serde(default)]
    pub transport: Option<AiTransport>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub cloud_warning_acknowledged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_transport_serializes_as_camel_case_with_a_kind_tag() {
        let transport = AiTransport::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".to_string(),
            model: "llama3.1".to_string(),
        };

        let value = serde_json::to_value(&transport).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "kind": "openAiCompatible",
                "baseUrl": "http://localhost:11434/v1",
                "model": "llama3.1",
            })
        );
    }

    #[test]
    fn ai_settings_defaults_to_no_transport_and_no_instructions() {
        let settings = AiSettings::default();

        assert_eq!(settings.transport, None);
        assert_eq!(settings.instructions, "");
        assert!(!settings.cloud_warning_acknowledged);
    }
}
