// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Generic line-by-line subprocess output, shared by commit hooks (`hooks::run`) and push
//! (`remote::run_git_streaming`) so both can forward a live transcript to the frontend
//! instead of only handing back a result once the process exits.

use std::io::{BufRead, BufReader};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// One line of output from a hook (or, for push, the underlying `git` process) — `hook` is a
/// label like `"pre-commit"`/`"commit-msg"`/`"post-commit"`/`"push"`, not necessarily a literal
/// hook name (push's transcript interleaves `git`'s own messages with whatever `pre-push`
/// prints, indistinguishably — see `remote::run_git_streaming`'s doc comment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutputLine {
    pub hook: String,
    pub stream: OutputStream,
    pub text: String,
}

/// Spawns `command` with piped stdout/stderr, reads both concurrently (one `std::thread` per
/// stream — simplest option for a `std::process::Command` child, and fine here since this
/// already runs on a `spawn_blocking` thread with no async runtime to hand the reads to),
/// calling `on_line` for each line as it arrives, and returns the exit status plus the full
/// transcript in the order `on_line` saw it. Ordering *within* a stream is preserved; ordering
/// *between* stdout and stderr is best-effort (two independently-scheduled reader threads, so a
/// stderr line can occasionally overtake a stdout line that was technically written first) —
/// still far closer to a live terminal transcript than the old `Command::output()`-based
/// approach, which only ever had stdout-then-stderr, in full, to work with. `on_line` runs on
/// the calling thread, not the reader threads, so it doesn't need to be `Send`.
pub fn stream_command(
    mut command: Command,
    hook: &str,
    on_line: &mut dyn FnMut(HookOutputLine),
) -> std::io::Result<(ExitStatus, String)> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout was piped above");
    let stderr = child.stderr.take().expect("stderr was piped above");

    let (tx, rx) = mpsc::channel::<(OutputStream, String)>();
    let stdout_tx = tx.clone();

    let stdout_thread = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if stdout_tx.send((OutputStream::Stdout, line)).is_err() {
                break;
            }
        }
    });
    let stderr_thread = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send((OutputStream::Stderr, line)).is_err() {
                break;
            }
        }
    });

    let mut combined = String::new();
    for (stream, text) in rx {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&text);
        on_line(HookOutputLine {
            hook: hook.to_string(),
            stream,
            text,
        });
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let status = child.wait()?;

    Ok((status, combined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_stdout_and_stderr_lines_preserving_order_within_each_stream() {
        // Two lines per stream, not one, so ordering *within* a stream is meaningfully
        // exercised — cross-stream order between independently-scheduled reader threads is
        // best-effort (see `stream_command`'s doc comment) and deliberately not asserted here.
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("echo out-one; echo err-one >&2; echo out-two; echo err-two >&2");

        let mut lines = Vec::new();
        let (status, _combined) =
            stream_command(command, "test-hook", &mut |line| lines.push(line)).unwrap();

        assert!(status.success());
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|l| l.hook == "test-hook"));

        let stdout_texts: Vec<&str> = lines
            .iter()
            .filter(|l| l.stream == OutputStream::Stdout)
            .map(|l| l.text.as_str())
            .collect();
        let stderr_texts: Vec<&str> = lines
            .iter()
            .filter(|l| l.stream == OutputStream::Stderr)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(stdout_texts, vec!["out-one", "out-two"]);
        assert_eq!(stderr_texts, vec!["err-one", "err-two"]);
    }

    #[test]
    fn reports_a_non_zero_exit_status() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("exit 1");

        let (status, _) = stream_command(command, "test-hook", &mut |_| {}).unwrap();

        assert!(!status.success());
    }
}
