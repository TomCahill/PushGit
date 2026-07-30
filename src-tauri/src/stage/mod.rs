// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Staging/unstaging including per-hunk and per-line staging, index read/write.

use std::collections::HashSet;
use std::path::Path;

use git2::{ApplyLocation, Diff, Oid, Repository};

use crate::diff::{Hunk, LineOrigin};
use crate::error::{PushGitError, PushGitResult};
use crate::hooks;

/// Stages a whole file: adds its current working-tree content to the index, or — if the
/// file no longer exists on disk — stages the deletion.
pub fn stage_file(repo: &Repository, path: &str) -> PushGitResult<()> {
    let mut index = repo.index()?;
    let workdir = repo.workdir().unwrap_or_else(|| repo.path());

    if workdir.join(path).exists() {
        index.add_path(Path::new(path))?;
    } else {
        index.remove_path(Path::new(path))?;
    }

    index.write()?;
    Ok(())
}

/// Unstages a whole file: resets its index entry back to HEAD's version (or removes it
/// from the index entirely if HEAD has no such path, i.e. it was newly added).
pub fn unstage_file(repo: &Repository, path: &str) -> PushGitResult<()> {
    let head = repo.head()?.peel(git2::ObjectType::Commit)?;
    repo.reset_default(Some(&head), [path])?;
    Ok(())
}

/// Applies a single hunk (from the *unstaged* diff) to the index — "stage this hunk"
/// without touching the rest of the file.
pub fn stage_hunk(repo: &Repository, path: &str, hunk: &Hunk) -> PushGitResult<()> {
    apply_hunk(repo, path, hunk, false, None)
}

/// Reverse-applies a single hunk (from the *staged* diff) to the index — "unstage this
/// hunk" without touching the rest of the file's staged changes.
pub fn unstage_hunk(repo: &Repository, path: &str, hunk: &Hunk) -> PushGitResult<()> {
    apply_hunk(repo, path, hunk, true, None)
}

/// Applies only the given lines of a hunk (from the *unstaged* diff) to the index — "stage
/// these lines" without staging the rest of the hunk. `line_indices` are positions into
/// `hunk.lines`; indices naming a context line are harmless (context is always kept) but
/// pointless, since only addition/deletion lines are meaningfully selectable. A no-op if
/// `line_indices` is empty.
pub fn stage_lines(
    repo: &Repository,
    path: &str,
    hunk: &Hunk,
    line_indices: &[usize],
) -> PushGitResult<()> {
    if line_indices.is_empty() {
        return Ok(());
    }
    apply_hunk(repo, path, hunk, false, Some(line_indices))
}

/// Reverse-applies only the given lines of a hunk (from the *staged* diff) to the index —
/// "unstage these lines" without touching the rest of the file's staged changes. Same
/// `line_indices` contract as [`stage_lines`].
pub fn unstage_lines(
    repo: &Repository,
    path: &str,
    hunk: &Hunk,
    line_indices: &[usize],
) -> PushGitResult<()> {
    if line_indices.is_empty() {
        return Ok(());
    }
    apply_hunk(repo, path, hunk, true, Some(line_indices))
}

fn apply_hunk(
    repo: &Repository,
    path: &str,
    hunk: &Hunk,
    reverse: bool,
    selected_lines: Option<&[usize]>,
) -> PushGitResult<()> {
    let patch_text = build_hunk_patch(path, hunk, reverse, selected_lines);
    let diff = Diff::from_buffer(patch_text.as_bytes())?;
    repo.apply(&diff, ApplyLocation::Index, None)?;
    Ok(())
}

/// Reconstructs a minimal valid unified-diff patch for exactly one hunk, so it can be
/// parsed back via `Diff::from_buffer` and applied to just the index — this is the
/// standard git2 technique for hunk-level staging (there's no dedicated "stage this hunk"
/// API; you apply a diff containing only that hunk).
///
/// `selected_lines`, when given, restricts which non-context lines actually take effect —
/// the same technique `git add -p` uses for staging a subset of a hunk: an unselected
/// addition is dropped from the patch entirely (so it isn't applied), and an unselected
/// deletion is downgraded to a context line (so it's kept rather than removed). Header
/// counts are recomputed from what's actually emitted; `old_start`/`new_start` (post
/// direction-swap) are left as reported, since nothing before this hunk shifts regardless
/// of which of *its own* lines are selected.
fn build_hunk_patch(
    path: &str,
    hunk: &Hunk,
    reverse: bool,
    selected_lines: Option<&[usize]>,
) -> String {
    let selected: Option<HashSet<usize>> = selected_lines.map(|s| s.iter().copied().collect());

    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;

    for (idx, line) in hunk.lines.iter().enumerate() {
        let origin = if reverse {
            reverse_origin(line.origin)
        } else {
            line.origin
        };
        let is_selected = selected.as_ref().is_none_or(|s| s.contains(&idx));

        let effective_origin = match origin {
            LineOrigin::Addition if !is_selected => None,
            LineOrigin::Deletion if !is_selected => Some(LineOrigin::Context),
            other => Some(other),
        };
        let Some(effective_origin) = effective_origin else {
            continue;
        };

        match effective_origin {
            LineOrigin::Context => {
                old_count += 1;
                new_count += 1;
            }
            LineOrigin::Addition => new_count += 1,
            LineOrigin::Deletion => old_count += 1,
        }

        let prefix = match effective_origin {
            LineOrigin::Addition => '+',
            LineOrigin::Deletion => '-',
            LineOrigin::Context => ' ',
        };
        body.push(prefix);
        body.push_str(&line.content);
        if !line.content.ends_with('\n') {
            body.push('\n');
        }
    }

    let (old_start, new_start) = if reverse {
        (hunk.new_start, hunk.old_start)
    } else {
        (hunk.old_start, hunk.new_start)
    };

    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));
    out.push_str(&format!("--- a/{path}\n"));
    out.push_str(&format!("+++ b/{path}\n"));
    out.push_str(&format!(
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    ));
    out.push_str(&body);

    out
}

fn reverse_origin(origin: LineOrigin) -> LineOrigin {
    match origin {
        LineOrigin::Addition => LineOrigin::Deletion,
        LineOrigin::Deletion => LineOrigin::Addition,
        LineOrigin::Context => LineOrigin::Context,
    }
}

/// Commits the current index tree. When `amend` is true, rewrites HEAD in place instead of
/// creating a new commit, pre-filling nothing itself — the caller supplies the (possibly
/// edited) message, for the "amend" MVP item. Otherwise, if a merge is
/// paused (`.git/MERGE_HEAD` present — `branch::merge_branch` left it there after a
/// conflicting 3-way merge), the merged-in commit(s) become extra parents and the merge
/// state is cleared, exactly like plain `git commit` finishing a conflicted merge; or, if a
/// cherry-pick is paused (`.git/CHERRY_PICK_HEAD` present — `branch::cherry_pick` left it
/// there after a conflict), the new commit keeps a single parent (HEAD, not a merge) but
/// reuses the cherry-picked commit's original author, matching plain `git cherry-pick
/// --continue`'s behavior — either way this is what lets the ordinary staging/commit flow
/// double as "finish this operation" once the user has resolved and staged the conflicts.
///
/// Unless `skip_hooks` (the "skip hooks" toggle — real `git commit
/// --no-verify`'s equivalent), `pre-commit` runs first and can reject the commit outright,
/// then `commit-msg` runs against `message` and can rewrite it — both via `hooks::` (see
/// that module's doc comment for why `git2` alone never does this). `post-commit` always
/// runs afterward regardless of `skip_hooks`, matching real git: `--no-verify` only ever
/// bypasses `pre-commit`/`commit-msg`.
pub fn commit(
    repo: &Repository,
    message: &str,
    amend: bool,
    skip_hooks: bool,
) -> PushGitResult<Oid> {
    if !skip_hooks {
        if let Some(rejection) = hooks::run_pre_commit(repo)? {
            return Err(rejection.into());
        }
    }
    let message = if skip_hooks {
        message.to_string()
    } else {
        hooks::run_commit_msg(repo, message)?.map_err(PushGitError::from)?
    };

    let mut index = repo.index()?;
    let tree = repo.find_tree(index.write_tree()?)?;
    let signature = repo.signature()?;

    let oid = if amend {
        let head_commit = repo.head()?.peel_to_commit()?;
        head_commit.amend(
            Some("HEAD"),
            Some(&signature),
            Some(&signature),
            None,
            Some(&message),
            Some(&tree),
        )?
    } else {
        let head_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let merge_parents = merge_head_commits(repo)?;
        let is_merge = !merge_parents.is_empty();
        let cherry_pick_source = cherry_pick_head_commit(repo)?;
        let is_cherry_pick = cherry_pick_source.is_some();

        let mut parents: Vec<&git2::Commit> = Vec::new();
        parents.extend(head_commit.iter());
        parents.extend(merge_parents.iter());

        let author = match &cherry_pick_source {
            Some(commit) => commit.author(),
            None => signature.clone(),
        };

        let oid = repo.commit(Some("HEAD"), &author, &signature, &message, &tree, &parents)?;

        if is_merge || is_cherry_pick {
            repo.cleanup_state()?;
        }

        oid
    };

    hooks::run_post_commit(repo);
    Ok(oid)
}

/// Discards a path's uncommitted changes, both staged and unstaged: for a file that exists
/// in HEAD, restores HEAD's content to both the working tree and the index in one step
/// (`git restore --staged --worktree <path>`'s combined effect — a GUI "discard" click
/// shouldn't leave a half-discarded staged/unstaged split behind); for a file HEAD has no
/// record of (newly added, whether or not it's already staged), removes it from disk and
/// the index entirely. Protected by the undo/redo stack at the
/// command layer — this is otherwise unrecoverable.
pub fn discard_file_changes(repo: &Repository, path: &str) -> PushGitResult<()> {
    let head_commit = repo.head()?.peel_to_commit()?;
    let head_tree = head_commit.tree()?;

    if head_tree.get_path(Path::new(path)).is_ok() {
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force().path(path);
        repo.checkout_tree(head_commit.as_object(), Some(&mut checkout))?;
        repo.reset_default(Some(head_commit.as_object()), [path])?;
    } else {
        let workdir = repo.workdir().ok_or_else(|| {
            PushGitError::Invalid("repository has no working directory".to_string())
        })?;
        let full_path = workdir.join(path);
        if full_path.exists() {
            std::fs::remove_file(full_path)?;
        }
        let mut index = repo.index()?;
        index.remove_path(Path::new(path))?;
        index.write()?;
    }
    Ok(())
}

/// Reads `.git/MERGE_HEAD` directly rather than via git2's `mergehead_foreach` (which needs
/// `&mut Repository`, forcing that everywhere `commit` is called) — it's a stable plumbing
/// file, one commit-ish oid per line. Empty (not an error) when no merge is in progress.
fn merge_head_commits(repo: &Repository) -> PushGitResult<Vec<git2::Commit<'_>>> {
    let Ok(contents) = std::fs::read_to_string(repo.path().join("MERGE_HEAD")) else {
        return Ok(Vec::new());
    };

    contents
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let oid = git2::Oid::from_str(line.trim())?;
            Ok(repo.find_commit(oid)?)
        })
        .collect()
}

/// Reads `.git/CHERRY_PICK_HEAD` the same way `merge_head_commits` reads `MERGE_HEAD` — a
/// single oid, one line, absent (not an error) when no cherry-pick is in progress.
fn cherry_pick_head_commit(repo: &Repository) -> PushGitResult<Option<git2::Commit<'_>>> {
    let Ok(contents) = std::fs::read_to_string(repo.path().join("CHERRY_PICK_HEAD")) else {
        return Ok(None);
    };
    let oid = git2::Oid::from_str(contents.trim())?;
    Ok(Some(repo.find_commit(oid)?))
}

/// The current HEAD commit's message, so the frontend can pre-fill the amend box per
/// the "amend" MVP item. `None` on an unborn HEAD (a brand-new repo with no
/// commits yet), where amending doesn't apply anyway.
pub fn head_commit_message(repo: &Repository) -> PushGitResult<Option<String>> {
    Ok(repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .map(|c| c.message().unwrap_or_default().to_string()))
}

/// The commit message template configured via git's `commit.template`, so the frontend can
/// pre-fill a fresh (non-amend) commit box the same way real `git commit` pre-fills its
/// editor. `Config::get_path`, not `get_string`, is what resolves
/// `~`/`~user` in the configured path the way git itself does for this key. An unset key, a
/// missing/unreadable file, or non-UTF-8 content all collapse to `Ok(None)` rather than an
/// error — none of them should ever block committing; this is a convenience prefill only.
pub fn commit_message_template(repo: &Repository) -> PushGitResult<Option<String>> {
    let config = repo.config()?;
    let Ok(path) = config.get_path("commit.template") else {
        return Ok(None);
    };
    Ok(std::fs::read_to_string(path).ok())
}

/// The raw `commit.template` path as configured, for a "This Repository" settings field to
/// display/edit — unlike `commit_message_template`
/// above, this returns the configured string itself, not `~`-expanded or read as file content.
/// An unset key collapses to `Ok(None)`, matching `commit_message_template`'s convention.
pub fn commit_template_path(repo: &Repository) -> PushGitResult<Option<String>> {
    let config = repo.config()?;
    Ok(config.get_string("commit.template").ok())
}

/// Sets (or, if `path` is `None`, clears) git's own `commit.template` key directly —
/// deliberately not cached in app-owned storage, so PushGit and the real `git` CLI always
/// agree on this repo's template. Clearing an
/// already-unset key is treated as success, not an error, since the end state is the same.
pub fn set_commit_template_path(repo: &Repository, path: Option<&str>) -> PushGitResult<()> {
    let mut config = repo.config()?;
    match path {
        Some(path) => config.set_str("commit.template", path)?,
        None => match config.remove("commit.template") {
            Ok(()) => {}
            Err(e) if e.code() == git2::ErrorCode::NotFound => {}
            Err(e) => return Err(e.into()),
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff;
    use crate::test_support::repo_init;
    use std::fs;

    fn is_staged(repo: &Repository, path: &str) -> bool {
        diff::diff_staged(repo)
            .unwrap()
            .iter()
            .any(|d| d.new_path.as_deref() == Some(path))
    }

    fn is_unstaged(repo: &Repository, path: &str) -> bool {
        diff::diff_unstaged(repo)
            .unwrap()
            .iter()
            .any(|d| d.new_path.as_deref() == Some(path) || d.old_path.as_deref() == Some(path))
    }

    #[test]
    fn stage_and_unstage_a_whole_file_round_trips() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();

        assert!(is_unstaged(&repo, "new.txt"));
        stage_file(&repo, "new.txt").unwrap();
        assert!(is_staged(&repo, "new.txt"));

        unstage_file(&repo, "new.txt").unwrap();
        assert!(!is_staged(&repo, "new.txt"));
        assert!(is_unstaged(&repo, "new.txt"));
    }

    fn commit_file(repo: &Repository, name: &str, content: &str) -> Oid {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &[&head])
            .unwrap()
    }

    #[test]
    fn stage_hunk_stages_only_that_hunk() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        let mut modified = lines.clone();
        modified[1] = "line 2 CHANGED".to_string();
        modified[17] = "line 18 CHANGED".to_string();
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();

        let unstaged = diff::diff_unstaged(&repo).unwrap();
        assert_eq!(unstaged.len(), 1);
        assert_eq!(
            unstaged[0].hunks.len(),
            2,
            "two well-separated edits should form two hunks"
        );

        stage_hunk(&repo, "f.txt", &unstaged[0].hunks[0]).unwrap();

        let staged = diff::diff_staged(&repo).unwrap();
        assert_eq!(staged.len(), 1);
        assert_eq!(
            staged[0].hunks.len(),
            1,
            "only the staged hunk should show up as staged"
        );

        let remaining_unstaged = diff::diff_unstaged(&repo).unwrap();
        assert_eq!(remaining_unstaged.len(), 1);
        assert_eq!(
            remaining_unstaged[0].hunks.len(),
            1,
            "the other hunk is still unstaged"
        );
    }

    #[test]
    fn unstage_hunk_reverses_a_staged_hunk() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        let mut modified = lines.clone();
        modified[1] = "line 2 CHANGED".to_string();
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();
        stage_file(&repo, "f.txt").unwrap();
        assert!(is_staged(&repo, "f.txt"));

        let staged = diff::diff_staged(&repo).unwrap();
        unstage_hunk(&repo, "f.txt", &staged[0].hunks[0]).unwrap();

        assert!(!is_staged(&repo, "f.txt"));
    }

    /// Reads the index's current staged content for `path` (stage 0 — the ordinary,
    /// non-conflicted entry), so line-selection tests can assert on exactly what did or
    /// didn't get staged rather than just the hunk count.
    fn staged_content(repo: &Repository, path: &str) -> String {
        let index = repo.index().unwrap();
        let entry = index.get_path(Path::new(path), 0).unwrap();
        let blob = repo.find_blob(entry.id).unwrap();
        String::from_utf8(blob.content().to_vec()).unwrap()
    }

    fn addition_indices(hunk: &Hunk) -> Vec<usize> {
        hunk.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.origin == LineOrigin::Addition)
            .map(|(i, _)| i)
            .collect()
    }

    fn deletion_indices(hunk: &Hunk) -> Vec<usize> {
        hunk.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.origin == LineOrigin::Deletion)
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn stage_lines_stages_only_the_selected_addition() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        // Two pure insertions close enough together to land in one hunk.
        let mut modified = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            modified.push(line.clone());
            if i == 2 {
                modified.push("NEW A".to_string());
            }
            if i == 5 {
                modified.push("NEW B".to_string());
            }
        }
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();

        let unstaged = diff::diff_unstaged(&repo).unwrap();
        assert_eq!(
            unstaged[0].hunks.len(),
            1,
            "nearby inserts should form one hunk"
        );
        let hunk = &unstaged[0].hunks[0];
        let additions = addition_indices(hunk);
        assert_eq!(additions.len(), 2, "exactly two inserted lines");
        assert!(deletion_indices(hunk).is_empty());

        stage_lines(&repo, "f.txt", hunk, &[additions[0]]).unwrap();

        let content = staged_content(&repo, "f.txt");
        assert!(content.contains("NEW A"));
        assert!(
            !content.contains("NEW B"),
            "unselected addition must stay unstaged"
        );

        let remaining_unstaged = diff::diff_unstaged(&repo).unwrap();
        assert_eq!(remaining_unstaged.len(), 1);
        let remaining_content: String = remaining_unstaged[0]
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.origin == LineOrigin::Addition)
            .map(|l| l.content.clone())
            .collect();
        assert!(
            remaining_content.contains("NEW B"),
            "the unselected insert is still pending in the working tree"
        );
    }

    #[test]
    fn unstage_lines_unstages_only_the_selected_addition() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        let mut modified = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            modified.push(line.clone());
            if i == 2 {
                modified.push("NEW A".to_string());
            }
            if i == 5 {
                modified.push("NEW B".to_string());
            }
        }
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();
        stage_file(&repo, "f.txt").unwrap();

        let staged = diff::diff_staged(&repo).unwrap();
        assert_eq!(staged[0].hunks.len(), 1);
        let hunk = &staged[0].hunks[0];
        let additions = addition_indices(hunk);
        assert_eq!(additions.len(), 2);

        unstage_lines(&repo, "f.txt", hunk, &[additions[0]]).unwrap();

        let content = staged_content(&repo, "f.txt");
        assert!(
            !content.contains("NEW A"),
            "selected addition must be unstaged"
        );
        assert!(
            content.contains("NEW B"),
            "unselected addition must stay staged"
        );
    }

    #[test]
    fn stage_lines_stages_only_the_selected_deletion() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        // Delete "line 4" and "line 7" — close enough (2 lines of context between them) to
        // land in one hunk, with no additions at all.
        let modified: Vec<String> = lines
            .iter()
            .filter(|l| l.as_str() != "line 4" && l.as_str() != "line 7")
            .cloned()
            .collect();
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();

        let unstaged = diff::diff_unstaged(&repo).unwrap();
        assert_eq!(
            unstaged[0].hunks.len(),
            1,
            "nearby deletes should form one hunk"
        );
        let hunk = &unstaged[0].hunks[0];
        let deletions = deletion_indices(hunk);
        assert_eq!(deletions.len(), 2);
        assert!(addition_indices(hunk).is_empty());

        stage_lines(&repo, "f.txt", hunk, &[deletions[0]]).unwrap();

        let content = staged_content(&repo, "f.txt");
        assert!(
            !content.contains("line 4"),
            "selected deletion must be staged"
        );
        assert!(
            content.contains("line 7"),
            "unselected deletion must stay unstaged"
        );
    }

    #[test]
    fn selecting_every_line_behaves_like_the_whole_hunk_stage() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        let mut modified = lines.clone();
        modified[3] = "line 4 CHANGED".to_string();
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();

        let unstaged = diff::diff_unstaged(&repo).unwrap();
        let hunk = &unstaged[0].hunks[0];
        let all_indices: Vec<usize> = (0..hunk.lines.len()).collect();

        stage_lines(&repo, "f.txt", hunk, &all_indices).unwrap();

        assert!(!is_unstaged(&repo, "f.txt"));
        let content = staged_content(&repo, "f.txt");
        assert!(content.contains("line 4 CHANGED"));
    }

    #[test]
    fn stage_lines_with_an_empty_selection_is_a_no_op() {
        let (dir, repo) = repo_init();
        let lines: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        commit_file(&repo, "f.txt", &format!("{}\n", lines.join("\n")));

        let mut modified = lines.clone();
        modified[3] = "line 4 CHANGED".to_string();
        fs::write(
            dir.path().join("f.txt"),
            format!("{}\n", modified.join("\n")),
        )
        .unwrap();

        let unstaged = diff::diff_unstaged(&repo).unwrap();
        let hunk = &unstaged[0].hunks[0];

        stage_lines(&repo, "f.txt", hunk, &[]).unwrap();

        assert!(!is_staged(&repo, "f.txt"));
    }

    #[test]
    fn commit_creates_a_new_commit_with_the_staged_tree() {
        let (dir, repo) = repo_init();
        let head_before = repo.head().unwrap().target().unwrap();
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let new_oid = commit(&repo, "add new.txt", false, false).unwrap();

        assert_ne!(new_oid, head_before);
        let head_after = repo.head().unwrap().target().unwrap();
        assert_eq!(head_after, new_oid);
        let commit_obj = repo.find_commit(new_oid).unwrap();
        assert_eq!(commit_obj.parent_id(0).unwrap(), head_before);
    }

    fn write_hook(repo: &Repository, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        let dir = repo.path().join("hooks");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn commit_is_rejected_by_a_failing_pre_commit_hook() {
        let (dir, repo) = repo_init();
        let head_before = repo.head().unwrap().target().unwrap();
        write_hook(
            &repo,
            "pre-commit",
            "#!/bin/sh\necho 'lint failed' >&2\nexit 1\n",
        );
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let err = commit(&repo, "add new.txt", false, false).unwrap_err();

        assert!(err.to_string().contains("lint failed"));
        assert_eq!(repo.head().unwrap().target().unwrap(), head_before);
    }

    #[test]
    fn commit_uses_the_message_rewritten_by_a_commit_msg_hook() {
        let (dir, repo) = repo_init();
        write_hook(
            &repo,
            "commit-msg",
            "#!/bin/sh\necho 'rewritten by hook' > \"$1\"\nexit 0\n",
        );
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let oid = commit(&repo, "original message", false, false).unwrap();

        assert_eq!(
            repo.find_commit(oid).unwrap().message(),
            Some("rewritten by hook\n")
        );
    }

    #[test]
    fn commit_with_skip_hooks_bypasses_a_rejecting_pre_commit_hook() {
        let (dir, repo) = repo_init();
        write_hook(&repo, "pre-commit", "#!/bin/sh\nexit 1\n");
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let oid = commit(&repo, "add new.txt", false, true).unwrap();

        assert_eq!(
            repo.find_commit(oid).unwrap().message(),
            Some("add new.txt")
        );
    }

    #[test]
    fn commit_runs_post_commit_even_when_skip_hooks_is_true() {
        let (dir, repo) = repo_init();
        let marker = dir.path().join("post-commit-ran");
        write_hook(
            &repo,
            "post-commit",
            &format!("#!/bin/sh\ntouch {}\n", marker.display()),
        );
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        commit(&repo, "add new.txt", false, true).unwrap();

        assert!(
            marker.exists(),
            "post-commit must run regardless of skip_hooks"
        );
    }

    #[test]
    fn amend_rewrites_head_instead_of_adding_a_parent() {
        let (dir, repo) = repo_init();
        let original_head = repo.head().unwrap().peel_to_commit().unwrap();
        let original_parent_count = original_head.parent_count();

        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        let amended_oid = commit(&repo, "amended message", true, false).unwrap();

        let amended = repo.find_commit(amended_oid).unwrap();
        assert_eq!(amended.parent_count(), original_parent_count);
        assert_eq!(amended.message(), Some("amended message"));
    }

    #[test]
    fn commit_finishes_a_conflicted_merge_as_a_real_merge_commit() {
        use crate::branch::{
            checkout_branch, create_branch, list_conflicts, merge_branch, repo_state, RepoState,
        };

        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();

        let main_tip = commit_file(&repo, "shared.txt", "main version\n");

        checkout_branch(&repo, "feature").unwrap();
        let feature_tip = commit_file(&repo, "shared.txt", "feature version\n");

        checkout_branch(&repo, "main").unwrap();
        merge_branch(&repo, "feature").unwrap();

        // Resolve via the ordinary staging flow — a real user would fix the conflict
        // markers in their own editor, then stage the file exactly like any other change.
        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        stage_file(&repo, "shared.txt").unwrap();

        let merge_commit_oid = commit(&repo, "Merge branch 'feature'", false, false).unwrap();

        let merge_commit = repo.find_commit(merge_commit_oid).unwrap();
        assert_eq!(merge_commit.parent_count(), 2);
        assert_eq!(merge_commit.parent_id(0).unwrap(), main_tip);
        assert_eq!(merge_commit.parent_id(1).unwrap(), feature_tip);
        assert!(list_conflicts(&repo).unwrap().is_empty());
        assert_eq!(repo_state(&repo), RepoState::Clean);
    }

    #[test]
    fn commit_finishes_a_conflicted_cherry_pick_preserving_the_original_author_as_a_single_parent()
    {
        use crate::branch::{
            checkout_branch, cherry_pick, create_branch, list_conflicts, repo_state,
            CherryPickOutcome, RepoState,
        };

        let (dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        create_branch(&repo, "feature", None).unwrap();
        checkout_branch(&repo, "feature").unwrap();

        fs::write(dir.path().join("shared.txt"), "feature version\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let author = git2::Signature::now("Ada Lovelace", "ada@example.com").unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let feature_commit = repo
            .commit(
                Some("HEAD"),
                &author,
                &author,
                "shared.txt",
                &tree,
                &[&head],
            )
            .unwrap();

        checkout_branch(&repo, "main").unwrap();
        let main_tip = commit_file(&repo, "shared.txt", "main version\n");

        let outcome = cherry_pick(&repo, &feature_commit.to_string()).unwrap();
        assert!(matches!(outcome, CherryPickOutcome::Conflicts { .. }));

        fs::write(dir.path().join("shared.txt"), "resolved\n").unwrap();
        stage_file(&repo, "shared.txt").unwrap();

        let new_oid = commit(&repo, "shared.txt", false, false).unwrap();

        let new_commit = repo.find_commit(new_oid).unwrap();
        assert_eq!(new_commit.parent_count(), 1);
        assert_eq!(new_commit.parent_id(0).unwrap(), main_tip);
        assert_eq!(new_commit.author().name(), Some("Ada Lovelace"));
        assert!(list_conflicts(&repo).unwrap().is_empty());
        assert_eq!(repo_state(&repo), RepoState::Clean);
        assert!(!repo.path().join("CHERRY_PICK_HEAD").exists());
    }

    #[test]
    fn head_commit_message_returns_the_current_head_s_message() {
        let (_dir, repo) = repo_init();
        assert_eq!(
            head_commit_message(&repo).unwrap(),
            Some("initial commit".to_string())
        );
    }

    #[test]
    fn discard_file_changes_reverts_a_tracked_file_to_head_including_staged_edits() {
        let (dir, repo) = repo_init();
        commit_file(&repo, "tracked.txt", "original\n");
        fs::write(dir.path().join("tracked.txt"), "staged change\n").unwrap();
        stage_file(&repo, "tracked.txt").unwrap();
        fs::write(dir.path().join("tracked.txt"), "unstaged on top\n").unwrap();

        discard_file_changes(&repo, "tracked.txt").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
            "original\n"
        );
        assert!(!is_staged(&repo, "tracked.txt"));
        assert!(!is_unstaged(&repo, "tracked.txt"));
    }

    #[test]
    fn discard_file_changes_removes_a_newly_added_file_entirely() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        stage_file(&repo, "new.txt").unwrap();

        discard_file_changes(&repo, "new.txt").unwrap();

        assert!(!dir.path().join("new.txt").exists());
        assert!(!is_staged(&repo, "new.txt"));
    }

    #[test]
    fn head_commit_message_is_none_on_an_unborn_head() {
        let td = tempfile::TempDir::new().unwrap();
        let mut opts = git2::RepositoryInitOptions::new();
        opts.initial_head("main");
        let repo = Repository::init_opts(td.path(), &opts).unwrap();

        assert_eq!(head_commit_message(&repo).unwrap(), None);
    }

    #[test]
    fn commit_message_template_reads_the_configured_file() {
        let (dir, repo) = repo_init();
        let template_path = dir.path().join("template.txt");
        fs::write(&template_path, "Summary\n\nBody text\n").unwrap();
        repo.config()
            .unwrap()
            .set_str("commit.template", template_path.to_str().unwrap())
            .unwrap();

        assert_eq!(
            commit_message_template(&repo).unwrap(),
            Some("Summary\n\nBody text\n".to_string())
        );
    }

    #[test]
    fn commit_message_template_is_none_when_unset() {
        let (_dir, repo) = repo_init();
        assert_eq!(commit_message_template(&repo).unwrap(), None);
    }

    #[test]
    fn commit_message_template_is_none_when_the_configured_file_is_missing() {
        let (dir, repo) = repo_init();
        repo.config()
            .unwrap()
            .set_str(
                "commit.template",
                dir.path().join("does-not-exist.txt").to_str().unwrap(),
            )
            .unwrap();

        assert_eq!(commit_message_template(&repo).unwrap(), None);
    }

    #[test]
    fn commit_template_path_is_none_when_unset() {
        let (_dir, repo) = repo_init();
        assert_eq!(commit_template_path(&repo).unwrap(), None);
    }

    #[test]
    fn set_commit_template_path_writes_the_configured_key() {
        let (_dir, repo) = repo_init();

        set_commit_template_path(&repo, Some("/some/template.txt")).unwrap();

        assert_eq!(
            commit_template_path(&repo).unwrap(),
            Some("/some/template.txt".to_string())
        );
    }

    #[test]
    fn set_commit_template_path_with_none_clears_an_existing_key() {
        let (_dir, repo) = repo_init();
        set_commit_template_path(&repo, Some("/some/template.txt")).unwrap();

        set_commit_template_path(&repo, None).unwrap();

        assert_eq!(commit_template_path(&repo).unwrap(), None);
    }

    #[test]
    fn set_commit_template_path_with_none_on_an_already_unset_key_succeeds() {
        let (_dir, repo) = repo_init();

        assert!(set_commit_template_path(&repo, None).is_ok());
    }
}
