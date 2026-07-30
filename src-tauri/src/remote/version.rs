// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! System `git` version check — PushGit shells
//! out to whatever `git` is on `$PATH` for network operations, so it has no way to know
//! whether that binary is patched against CVE-2024-32002 unless it checks.

use serde::Serialize;
use tokio::process::Command;

use crate::error::{PushGitError, PushGitResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitVersionCheck {
    pub version: String,
    /// False means the installed `git` predates the CVE-2024-32002 fix for its release
    /// branch — PushGit should warn, not silently proceed.
    pub is_patched: bool,
}

pub async fn check_git_version() -> PushGitResult<GitVersionCheck> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|e| PushGitError::Subprocess {
            command: "git --version".to_string(),
            message: e.to_string(),
        })?;

    let text = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_version(&text).ok_or_else(|| {
        PushGitError::Invalid(format!("could not parse `git --version` output: {text:?}"))
    })?;

    Ok(GitVersionCheck {
        version: format!("{}.{}.{}", parsed.0, parsed.1, parsed.2),
        is_patched: is_patched(parsed),
    })
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let rest = text.trim().strip_prefix("git version ")?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    // Some distros append suffixes to the patch component (e.g. "1.windows.1"); only the
    // leading digits matter here.
    let patch_str = parts.next()?;
    let digits: String = patch_str
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let patch = digits.parse().ok()?;
    Some((major, minor, patch))
}

/// The minimum patched point-release per affected branch, per the CVE-2024-32002 advisory.
/// Branches not listed here are either newer (assumed patched)
/// or older than any maintained branch that received the fix (assumed unpatched).
fn is_patched((major, minor, patch): (u32, u32, u32)) -> bool {
    match (major, minor) {
        (2, 45) => patch >= 1,
        (2, 44) => patch >= 1,
        (2, 43) => patch >= 4,
        (2, 42) => patch >= 2,
        (2, 41) => patch >= 1,
        (2, 40) => patch >= 2,
        (2, 39) => patch >= 4,
        (2, m) if m > 45 => true,
        (m, _) if m > 2 => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_standard_version_string() {
        assert_eq!(parse_version("git version 2.43.0\n"), Some((2, 43, 0)));
    }

    #[test]
    fn parses_a_version_with_a_platform_suffix() {
        assert_eq!(
            parse_version("git version 2.39.2.windows.1\n"),
            Some((2, 39, 2))
        );
    }

    #[test]
    fn rejects_unparseable_output() {
        assert_eq!(parse_version("not git at all"), None);
    }

    #[test]
    fn exactly_the_patched_boundary_counts_as_patched() {
        assert!(is_patched((2, 43, 4)));
        assert!(!is_patched((2, 43, 3)));
    }

    #[test]
    fn versions_newer_than_the_advisory_list_are_assumed_patched() {
        assert!(is_patched((2, 46, 0)));
        assert!(is_patched((3, 0, 0)));
    }

    #[test]
    fn versions_older_than_any_patched_branch_are_unpatched() {
        assert!(!is_patched((2, 38, 5)));
    }

    #[tokio::test]
    async fn check_git_version_runs_against_the_real_installed_git() {
        // Smoke test only — this machine's actual git version is unknown, so we can only
        // assert the check runs and returns a well-formed version string.
        let result = check_git_version().await.unwrap();
        assert!(result.version.split('.').count() == 3);
    }
}
