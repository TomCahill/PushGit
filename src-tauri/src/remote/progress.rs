// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Parses `git`'s own stderr progress output (`Receiving objects:  45% (450/1000), 1.20
//! MiB | 500 KiB/s`) into structured updates for the frontend.
//! Pure/allocation-light and dependency-free on purpose — this runs on every progress
//! line a fetch/pull/push subprocess emits, and the shape git uses (`"<phase>: NN%
//! (a/b)[, extra]"`) is simple enough not to need a regex crate for it.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProgress {
    /// git's own phase label verbatim, e.g. "Receiving objects", "Resolving deltas".
    pub phase: String,
    pub percent: u8,
    pub current: Option<u64>,
    pub total: Option<u64>,
}

/// Parses one line of `git` stderr progress output, or `None` if it doesn't match the
/// `"<phase>: NN% (a/b)"` shape — a non-matching line (a plain "remote: ..." message, a
/// final summary line, etc.) simply isn't forwarded as structured progress; the caller
/// still captures raw stderr separately for error reporting on failure.
pub fn parse_progress_line(line: &str) -> Option<RemoteProgress> {
    let line = line.trim();
    let (phase, rest) = line.split_once(':')?;
    let phase = phase.trim();
    if phase.is_empty() {
        return None;
    }

    let rest = rest.trim();
    let percent_end = rest.find('%')?;
    let percent: u8 = rest[..percent_end].trim().parse().ok()?;

    let (current, total) = match (rest.find('('), rest.find(')')) {
        (Some(open), Some(close)) if open < close => {
            let inner = &rest[open + 1..close];
            match inner.split_once('/') {
                Some((c, t)) => (c.trim().parse().ok(), t.trim().parse().ok()),
                None => (None, None),
            }
        }
        _ => (None, None),
    };

    Some(RemoteProgress {
        phase: phase.to_string(),
        percent,
        current,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_standard_progress_line_with_counts() {
        let progress =
            parse_progress_line("Receiving objects:  45% (450/1000), 1.20 MiB | 500 KiB/s")
                .unwrap();

        assert_eq!(progress.phase, "Receiving objects");
        assert_eq!(progress.percent, 45);
        assert_eq!(progress.current, Some(450));
        assert_eq!(progress.total, Some(1000));
    }

    #[test]
    fn parses_a_completed_phase_line() {
        let progress = parse_progress_line("Resolving deltas: 100% (200/200), done.").unwrap();

        assert_eq!(progress.phase, "Resolving deltas");
        assert_eq!(progress.percent, 100);
        assert_eq!(progress.current, Some(200));
        assert_eq!(progress.total, Some(200));
    }

    #[test]
    fn parses_a_percent_only_line_with_no_counts() {
        let progress = parse_progress_line("Compressing objects: 100%").unwrap();

        assert_eq!(progress.percent, 100);
        assert_eq!(progress.current, None);
        assert_eq!(progress.total, None);
    }

    #[test]
    fn returns_none_for_a_line_with_no_colon() {
        assert!(parse_progress_line("done.").is_none());
    }

    #[test]
    fn returns_none_for_a_remote_message_with_no_percent() {
        assert!(parse_progress_line("remote: Enumerating objects: 5, done.").is_none());
    }

    #[test]
    fn returns_none_for_an_empty_line() {
        assert!(parse_progress_line("").is_none());
        assert!(parse_progress_line("   ").is_none());
    }
}
