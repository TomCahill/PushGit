// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Builds the prompt sent to the model from the same staged-diff data
//! `diff::diff_staged` already computes for the staging UI — no second diff-collection code
//! path. The base system prompt is fixed, not a placeholder: single-line-Conventional-Commit-
//! subject-only by design (see the feature plan's "Decided scope" note on why the description
//! field stays empty from generation), and tuned to summarize the whole diff rather than
//! fixate on whichever file/hunk happens to be largest.

use crate::diff::{FileDiff, Hunk, LineOrigin};

const SYSTEM_PROMPT: &str = "You are an expert developer. Read the following git diff and generate a Conventional Commit message.\nConsider the diff as a whole: summarize the overall intent across every file touched, weighing each roughly equally rather than fixating on whichever single file or hunk happens to be largest.\nA line reading \"[dependency lockfile - diff omitted; ...]\" marks a machine-generated lockfile update with no hand-written content - mention it only if it is the only change in the diff, never let it dominate the summary.\nFormat: <type>(<scope>): <subject>\nDo NOT include any explanations, markdown, or conversational text. Output ONLY the commit message.";

/// Diffs above this are truncated at the last complete file/hunk boundary before the cap —
/// very large diffs are rare for a single commit, and a truncated-but-present diff still
/// gives a model useful signal. A mid-line cutoff risks severing a `+`/`-` line mid-token and
/// feeding the model a syntactically broken half-hunk, hence the boundary search rather than
/// a raw byte offset.
const MAX_DIFF_BYTES: usize = 32 * 1024;

/// Basenames of common dependency lockfiles (not an exhaustive list) — machine-generated,
/// high-volume, and low-signal for "what did this commit actually do," so their diffs are
/// summarized rather than included in full. A large lockfile update would otherwise dominate
/// both the model's attention and the truncation budget ahead of the hand-written changes
/// that actually motivated the commit.
const LOCKFILE_BASENAMES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "composer.lock",
    "Gemfile.lock",
    "poetry.lock",
    "Pipfile.lock",
    "go.sum",
    "mix.lock",
    "flake.lock",
    "packages.lock.json",
    "pubspec.lock",
    "Podfile.lock",
];

fn display_path(file: &FileDiff) -> &str {
    file.new_path
        .as_deref()
        .or(file.old_path.as_deref())
        .unwrap_or("")
}

fn is_lockfile(path: &str) -> bool {
    let basename = path.rsplit('/').next().unwrap_or(path);
    LOCKFILE_BASENAMES.contains(&basename)
}

pub fn build_prompt(staged: &[FileDiff], instructions: &str) -> String {
    let mut prompt = SYSTEM_PROMPT.to_string();
    if !instructions.trim().is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(instructions.trim());
    }
    prompt.push_str("\n\nDiff:\n");
    prompt.push_str(&truncated_diff_text(staged));
    prompt
}

fn render_file_header(file: &FileDiff) -> String {
    let display_path = display_path(file);
    let mut text = format!("diff --git a/{display_path} b/{display_path}\n");
    if file.is_binary {
        text.push_str("Binary files differ\n");
    } else {
        let old_path = file.old_path.as_deref().unwrap_or("/dev/null");
        let new_path = file.new_path.as_deref().unwrap_or("/dev/null");
        text.push_str(&format!("--- a/{old_path}\n+++ b/{new_path}\n"));
    }
    text
}

fn render_hunk(hunk: &Hunk) -> String {
    let mut text = hunk.header.clone();
    text.push('\n');
    for line in &hunk.lines {
        let prefix = match line.origin {
            LineOrigin::Addition => '+',
            LineOrigin::Deletion => '-',
            LineOrigin::Context => ' ',
        };
        text.push(prefix);
        text.push_str(&line.content);
        if !line.content.ends_with('\n') {
            text.push('\n');
        }
    }
    text
}

/// Replaces a lockfile's hunks entirely — the diff itself is noise, but the insertion/
/// deletion counts still tell the model "dependencies changed" without spending prompt
/// budget or attention on the actual version/hash churn.
fn render_lockfile_body(file: &FileDiff) -> String {
    format!(
        "[dependency lockfile - diff omitted; +{} -{} lines]\n",
        file.insertions, file.deletions
    )
}

fn render_file_body(file: &FileDiff) -> String {
    if file.is_binary {
        return String::new(); // already noted in the header
    }
    if is_lockfile(display_path(file)) {
        return render_lockfile_body(file);
    }
    file.hunks.iter().map(render_hunk).collect()
}

fn render_unified_diff(files: &[FileDiff]) -> String {
    let mut text = String::new();
    for file in files {
        text.push_str(&render_file_header(file));
        text.push_str(&render_file_body(file));
    }
    text
}

/// Builds the truncated text incrementally, file by file and hunk by hunk, stopping before
/// any unit that would push past the cap — rather than rendering the full text and then
/// searching for a byte-offset boundary within it (fragile: a hunk's own opening `@@` line
/// can appear well before that hunk's content ends, which a naive string search can't tell
/// apart from a boundary *after* a complete hunk). This way a partial hunk/file is never
/// possible by construction. The very first header/hunk is always kept even if it alone
/// exceeds the cap, so a truncated-but-present diff never degrades to nothing. A lockfile's
/// body is one atomic summary line (see `render_lockfile_body`), not truncated hunk-by-hunk
/// like a normal file's.
fn truncated_diff_text(files: &[FileDiff]) -> String {
    let mut out = String::new();
    let mut truncated = false;

    'files: for file in files {
        let header = render_file_header(file);
        if !out.is_empty() && out.len() + header.len() > MAX_DIFF_BYTES {
            truncated = true;
            break;
        }
        out.push_str(&header);

        if file.is_binary {
            continue;
        }
        if is_lockfile(display_path(file)) {
            let summary = render_lockfile_body(file);
            if !out.is_empty() && out.len() + summary.len() > MAX_DIFF_BYTES {
                truncated = true;
                break;
            }
            out.push_str(&summary);
            continue;
        }

        for hunk in &file.hunks {
            let hunk_text = render_hunk(hunk);
            if !out.is_empty() && out.len() + hunk_text.len() > MAX_DIFF_BYTES {
                truncated = true;
                break 'files;
            }
            out.push_str(&hunk_text);
        }
    }

    if !truncated {
        return out;
    }
    let omitted = render_unified_diff(files).len() - out.len();
    format!("{out}\n\n[diff truncated; {omitted} bytes omitted]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileStatus, Hunk, Line};

    fn file(path: &str, hunks: Vec<Hunk>) -> FileDiff {
        FileDiff {
            old_path: Some(path.to_string()),
            new_path: Some(path.to_string()),
            status: FileStatus::Modified,
            is_binary: false,
            hunks,
            insertions: 0,
            deletions: 0,
        }
    }

    fn hunk(header: &str, lines: Vec<(LineOrigin, &str)>) -> Hunk {
        Hunk {
            header: header.to_string(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: lines
                .into_iter()
                .map(|(origin, content)| Line {
                    origin,
                    content: content.to_string(),
                    old_lineno: None,
                    new_lineno: None,
                })
                .collect(),
        }
    }

    #[test]
    fn build_prompt_includes_the_fixed_system_prompt() {
        let prompt = build_prompt(&[], "");
        assert!(prompt.starts_with(SYSTEM_PROMPT));
    }

    #[test]
    fn system_prompt_tells_the_model_to_weigh_the_whole_diff_and_explains_the_lockfile_marker() {
        assert!(SYSTEM_PROMPT.contains("as a whole"));
        assert!(SYSTEM_PROMPT.contains("dependency lockfile"));
    }

    #[test]
    fn build_prompt_appends_custom_instructions_after_the_system_prompt() {
        let prompt = build_prompt(&[], "Always mention ABC-123.");
        assert!(prompt.contains("Always mention ABC-123."));
        assert!(prompt.find(SYSTEM_PROMPT).unwrap() < prompt.find("Always mention").unwrap());
    }

    #[test]
    fn build_prompt_omits_the_instructions_block_when_empty() {
        let with_blank = build_prompt(&[], "   ");
        let without = build_prompt(&[], "");
        assert_eq!(with_blank, without);
    }

    #[test]
    fn renders_a_unified_diff_with_hunk_and_line_prefixes() {
        let files = vec![file(
            "src/main.rs",
            vec![hunk(
                "@@ -1,2 +1,3 @@",
                vec![
                    (LineOrigin::Context, "fn main() {\n"),
                    (LineOrigin::Addition, "    println!(\"hi\");\n"),
                    (LineOrigin::Deletion, "    todo!();\n"),
                ],
            )],
        )];

        let text = render_unified_diff(&files);

        assert!(text.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(text.contains("--- a/src/main.rs\n+++ b/src/main.rs"));
        assert!(text.contains("@@ -1,2 +1,3 @@\n"));
        assert!(text.contains("+    println!(\"hi\");\n"));
        assert!(text.contains("-    todo!();\n"));
    }

    #[test]
    fn binary_files_render_a_placeholder_with_no_hunks() {
        let files = vec![FileDiff {
            old_path: Some("image.png".to_string()),
            new_path: Some("image.png".to_string()),
            status: FileStatus::Modified,
            is_binary: true,
            hunks: Vec::new(),
            insertions: 0,
            deletions: 0,
        }];

        let text = render_unified_diff(&files);

        assert!(text.contains("Binary files differ"));
    }

    #[test]
    fn a_lockfile_diff_is_summarized_instead_of_included_in_full() {
        let files = vec![FileDiff {
            old_path: Some("Cargo.lock".to_string()),
            new_path: Some("Cargo.lock".to_string()),
            status: FileStatus::Modified,
            is_binary: false,
            hunks: vec![hunk(
                "@@ -100,6 +100,42 @@",
                vec![(LineOrigin::Addition, "checksum = \"deadbeef\"\n")],
            )],
            insertions: 40,
            deletions: 4,
        }];

        let text = render_unified_diff(&files);

        assert!(text.contains("diff --git a/Cargo.lock b/Cargo.lock"));
        assert!(text.contains("[dependency lockfile - diff omitted; +40 -4 lines]"));
        assert!(!text.contains("checksum = \"deadbeef\""));
    }

    #[test]
    fn a_nested_lockfile_is_recognized_by_basename() {
        let files = vec![FileDiff {
            old_path: Some("crates/sub/Cargo.lock".to_string()),
            new_path: Some("crates/sub/Cargo.lock".to_string()),
            status: FileStatus::Modified,
            is_binary: false,
            hunks: vec![hunk("@@ -1,1 +1,1 @@", vec![(LineOrigin::Addition, "x\n")])],
            insertions: 1,
            deletions: 0,
        }];

        let text = render_unified_diff(&files);

        assert!(text.contains("[dependency lockfile - diff omitted;"));
    }

    #[test]
    fn a_file_that_merely_contains_lock_in_its_name_is_not_treated_as_a_lockfile() {
        let files = vec![file(
            "src/lock_manager.rs",
            vec![hunk("@@ -1,1 +1,1 @@", vec![(LineOrigin::Addition, "x\n")])],
        )];

        let text = render_unified_diff(&files);

        assert!(!text.contains("dependency lockfile"));
        assert!(text.contains("+x\n"));
    }

    #[test]
    fn a_huge_lockfile_never_consumes_the_truncation_budget_ahead_of_a_real_change() {
        // A Cargo.lock update easily exceeds MAX_DIFF_BYTES on its own, and sorts ahead of
        // most `src/` paths alphabetically — before summarization, this would have consumed
        // the entire truncation budget and cut `real_change.rs` out of the prompt entirely.
        let big_line = "x".repeat(2000);
        let huge_lockfile_hunks: Vec<Hunk> = (0..50)
            .map(|i| {
                hunk(
                    &format!("@@ -{i},1 +{i},1 @@"),
                    vec![(LineOrigin::Addition, &format!("{big_line}\n"))],
                )
            })
            .collect();
        assert!(huge_lockfile_hunks.len() * big_line.len() > MAX_DIFF_BYTES);

        let files = vec![
            FileDiff {
                old_path: Some("Cargo.lock".to_string()),
                new_path: Some("Cargo.lock".to_string()),
                status: FileStatus::Modified,
                is_binary: false,
                hunks: huge_lockfile_hunks,
                insertions: 2500,
                deletions: 10,
            },
            file(
                "src/real_change.rs",
                vec![hunk(
                    "@@ -1,1 +1,1 @@",
                    vec![(LineOrigin::Addition, "meaningful line\n")],
                )],
            ),
        ];

        let text = truncated_diff_text(&files);

        assert!(!text.contains(&big_line)); // the huge hunk content never appears at all
        assert!(text.contains("[dependency lockfile - diff omitted; +2500 -10 lines]"));
        assert!(text.contains("src/real_change.rs"));
        assert!(text.contains("meaningful line"));
        assert!(!text.contains("truncated")); // nothing needed truncating once summarized
    }

    #[test]
    fn truncation_cuts_at_the_last_complete_hunk_boundary_before_the_cap() {
        let big_line = "x".repeat(1000);
        let files: Vec<FileDiff> = (0..50)
            .map(|i| {
                file(
                    &format!("file{i}.txt"),
                    vec![hunk(
                        "@@ -1,1 +1,1 @@",
                        vec![(LineOrigin::Addition, &format!("{big_line}\n"))],
                    )],
                )
            })
            .collect();

        let text = truncated_diff_text(&files);

        assert!(text.len() < render_unified_diff(&files).len());
        assert!(text.contains("[diff truncated;"));
        // Every hunk header that made it into the kept output is followed by its full line
        // content — no hunk is ever cut mid-content.
        let before_marker = text.split("\n\n[diff truncated;").next().unwrap();
        for hunk_block in before_marker.split("@@ -1,1 +1,1 @@\n").skip(1) {
            assert!(hunk_block.starts_with(&format!("+{big_line}\n")));
        }
    }

    #[test]
    fn truncation_keeps_at_least_the_first_header_even_if_it_alone_exceeds_the_cap() {
        let files = vec![file(
            &"a".repeat(MAX_DIFF_BYTES * 2),
            vec![hunk("@@ -1,1 +1,1 @@", vec![(LineOrigin::Addition, "x\n")])],
        )];

        let text = truncated_diff_text(&files);

        assert!(text.contains("diff --git"));
        assert!(text.contains("[diff truncated;"));
    }

    #[test]
    fn a_diff_at_or_under_the_cap_is_not_truncated() {
        let files = vec![file(
            "small.txt",
            vec![hunk(
                "@@ -1,1 +1,1 @@",
                vec![(LineOrigin::Addition, "hello\n")],
            )],
        )];

        let text = truncated_diff_text(&files);

        assert!(!text.contains("truncated"));
    }
}
