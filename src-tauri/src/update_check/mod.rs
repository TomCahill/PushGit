// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Checks GitHub's public releases API for a newer PushGit release. The one deliberate
//! exception to "PushGit never phones home" (`SPEC.md`'s design principles table) — see
//! `.private/feature/update-notification/PLAN.md` for the reversal rationale. A single
//! unauthenticated GET, no identifying data sent beyond default HTTP headers, opt-out via
//! `config::AppConfig::check_for_updates_enabled`.

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::error::{PushGitError, PushGitResult};

const GITHUB_API_BASE_URL: &str = "https://api.github.com";
const RELEASES_PATH: &str = "/repos/TomCahill/PushGit/releases/latest";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub published_at: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    published_at: String,
}

/// `current`/`latest` may or may not have a leading `v` (GitHub tags conventionally do,
/// `CARGO_PKG_VERSION` never does) — stripped before parsing either way. A tag that isn't
/// valid semver fails closed (`false`, never nags about a malformed release tag).
pub fn is_newer(current: &str, latest: &str) -> bool {
    let (Ok(current), Ok(latest)) = (
        Version::parse(current.trim_start_matches('v')),
        Version::parse(latest.trim_start_matches('v')),
    ) else {
        return false;
    };
    latest > current
}

fn parse_latest_release(body: &str) -> PushGitResult<ReleaseInfo> {
    let release: GitHubRelease = serde_json::from_str(body)
        .map_err(|e| PushGitError::Invalid(format!("malformed GitHub release response: {e}")))?;
    Ok(ReleaseInfo {
        version: release.tag_name.trim_start_matches('v').to_string(),
        url: release.html_url,
        published_at: release.published_at,
    })
}

/// `base_url` is `GITHUB_API_BASE_URL` in production and a `wiremock` server URL in tests —
/// same "thread the base URL through for testability" shape as `ai::anthropic::stream`.
pub async fn fetch_latest_release(base_url: &str) -> PushGitResult<ReleaseInfo> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            RELEASES_PATH
        ))
        .header("User-Agent", "PushGit")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| PushGitError::Invalid(format!("update check request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(PushGitError::Invalid(format!(
            "update check failed: GitHub returned {}",
            response.status()
        )));
    }

    let body = response
        .text()
        .await
        .map_err(|e| PushGitError::Invalid(format!("update check request failed: {e}")))?;
    parse_latest_release(&body)
}

/// The actual production entry point — checks GitHub and returns `Some` only when the release
/// found there is newer than the running app. Collapses every failure mode (offline, GitHub
/// down, malformed response) to `None`: there is nothing more useful a caller can do with "the
/// check itself failed" than with "no update available," so the distinction isn't surfaced.
pub async fn check_for_update(current_version: &str) -> Option<ReleaseInfo> {
    let release = fetch_latest_release(GITHUB_API_BASE_URL).await.ok()?;
    is_newer(current_version, &release.version).then_some(release)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn is_newer_true_for_a_greater_version() {
        assert!(is_newer("1.2.0", "1.3.0"));
        assert!(is_newer("1.2.0", "2.0.0"));
    }

    #[test]
    fn is_newer_false_for_equal_or_lesser_versions() {
        assert!(!is_newer("1.2.0", "1.2.0"));
        assert!(!is_newer("1.3.0", "1.2.0"));
    }

    #[test]
    fn is_newer_strips_leading_v_on_either_side() {
        assert!(is_newer("v1.2.0", "v1.3.0"));
        assert!(is_newer("1.2.0", "v1.3.0"));
        assert!(!is_newer("v1.3.0", "1.2.0"));
    }

    #[test]
    fn is_newer_fails_closed_on_malformed_versions() {
        assert!(!is_newer("1.2.0", "not-a-version"));
        assert!(!is_newer("not-a-version", "1.3.0"));
    }

    #[test]
    fn parse_latest_release_extracts_the_fields_used() {
        let body = r#"{
            "tag_name": "v1.3.0",
            "html_url": "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0",
            "published_at": "2026-08-01T00:00:00Z",
            "other_field_we_ignore": 42
        }"#;
        let release = parse_latest_release(body).unwrap();
        assert_eq!(
            release,
            ReleaseInfo {
                version: "1.3.0".to_string(),
                url: "https://github.com/TomCahill/PushGit/releases/tag/v1.3.0".to_string(),
                published_at: "2026-08-01T00:00:00Z".to_string(),
            }
        );
    }

    #[test]
    fn parse_latest_release_rejects_malformed_json() {
        assert!(parse_latest_release("not json").is_err());
    }

    #[tokio::test]
    async fn fetch_latest_release_parses_a_successful_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(RELEASES_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"tag_name":"v1.3.0","html_url":"https://example.com/r","published_at":"2026-08-01T00:00:00Z"}"#,
            ))
            .mount(&server)
            .await;

        let release = fetch_latest_release(&server.uri()).await.unwrap();
        assert_eq!(release.version, "1.3.0");
    }

    #[tokio::test]
    async fn fetch_latest_release_errors_on_non_success_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(RELEASES_PATH))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        assert!(fetch_latest_release(&server.uri()).await.is_err());
    }

    #[tokio::test]
    async fn fetch_latest_release_errors_on_malformed_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(RELEASES_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        assert!(fetch_latest_release(&server.uri()).await.is_err());
    }
}
