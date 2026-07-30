// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Filesystem watcher (`notify` crate) with `notify-debouncer-full` to coalesce bursts;
//! filters out `.git/**` noise except the specific paths that matter (`HEAD`, `refs/**`,
//! `index`, `MERGE_HEAD`) and out gitignored working-tree paths (build output, dependency
//! dirs, etc.) so external commits, branch switches, and stashes are detected without
//! triggering on PushGit's own internal writes, a build tool's churn under `target/` or
//! `node_modules/`, or causing refresh loops.
//!
//! One further wrinkle found by hand: the underlying watcher has been observed to
//! re-report the same `.git/HEAD`/`.git/index`/`.git/refs/**` paths as "changed" repeatedly
//! with provably unchanged mtimes — a self-sustaining loop, since every refresh those
//! trigger reads those same paths right back. [`git_state_signature`] guards specifically
//! against that: a `.git`-internal path is only treated as a real change if the repo's
//! actual resolved state (HEAD target, every ref's target, `MERGE_HEAD` presence, the
//! index's mtime/size) differs from the last time we checked. Ordinary working-tree edits
//! bypass this — they're not the thing that was misfiring — so they still refresh
//! immediately.

use std::path::Path;
use std::time::Duration;

use git2::Repository;
use notify_debouncer_full::{
    new_debouncer,
    notify::{RecommendedWatcher, RecursiveMode},
    DebounceEventResult, Debouncer, RecommendedCache,
};

use crate::error::{PushGitError, PushGitResult};

/// Decides whether a changed path (relative to the repository root) should trigger a
/// refresh. Inside `.git/`, only the paths that reflect user-visible state changes are —
/// the vast majority of `.git/**` writes (loose objects, packs, logs) are internal
/// bookkeeping no view in the app actually depends on. Outside `.git/`, an ordinary
/// working-tree edit is relevant unless `.gitignore` (or `.git/info/exclude`) says
/// otherwise — a gitignored path is, by definition, not something any view renders.
pub fn is_relevant_change(repo: &Repository, repo_relative_path: &Path) -> bool {
    let Ok(git_relative) = repo_relative_path.strip_prefix(".git") else {
        return !repo.is_path_ignored(repo_relative_path).unwrap_or(false);
    };

    let git_relative = git_relative.to_string_lossy().replace('\\', "/");
    git_relative == "HEAD"
        || git_relative == "index"
        || git_relative == "MERGE_HEAD"
        || git_relative.starts_with("refs/")
}

/// A cheap snapshot of exactly the git-internal state `is_relevant_change` cares about:
/// HEAD's resolved target, every ref's target, whether a merge is paused, and the index
/// file's mtime/size (its content isn't worth reading here — size+mtime already changes on
/// any real stage/unstage). `None` if the repo can't be opened right now (mid-operation);
/// treated as "unknown, assume changed" by the caller rather than silently swallowing a
/// real change.
fn git_state_signature(repo_path: &Path) -> Option<String> {
    let repo = Repository::open(repo_path).ok()?;

    let head_target = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|oid| oid.to_string());

    let mut refs: Vec<String> = repo
        .references()
        .ok()?
        .flatten()
        .filter_map(|r| Some(format!("{}@{}", r.name()?, r.target()?)))
        .collect();
    refs.sort();

    let merge_head_present = repo.path().join("MERGE_HEAD").exists();
    let index_stat = std::fs::metadata(repo.path().join("index"))
        .ok()
        .map(|m| (m.len(), m.modified().ok()));

    Some(format!(
        "{head_target:?}|{}|{merge_head_present}|{index_stat:?}",
        refs.join(",")
    ))
}

fn is_git_internal_path(repo_relative_path: &Path) -> bool {
    repo_relative_path.strip_prefix(".git").is_ok()
}

/// Starts watching `repo_path` recursively, calling `on_relevant_change` (debounced) once
/// per batch that contains a relevant working-tree change, or a relevant `.git`-internal
/// path whose [`git_state_signature`] actually differs from last time.
/// The returned `Debouncer` must be kept alive for as long as watching should continue —
/// dropping it stops the watch.
pub fn start_watching(
    repo_path: &Path,
    mut on_relevant_change: impl FnMut() + Send + 'static,
) -> PushGitResult<Debouncer<RecommendedWatcher, RecommendedCache>> {
    let repo_path_owned = repo_path.to_path_buf();
    let mut last_git_state = git_state_signature(&repo_path_owned);

    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: DebounceEventResult| {
            let Ok(events) = result else { return };
            // Re-opened per batch rather than held across the debounce window: cheap
            // (mmap, no object-db scan) and avoids keeping a `Repository` handle alive
            // indefinitely on a thread the caller doesn't control.
            let repo = Repository::open(&repo_path_owned).ok();

            let mut working_tree_change = false;
            let mut git_internal_change = false;

            for path in events.iter().flat_map(|event| &event.paths) {
                let Ok(relative) = path.strip_prefix(&repo_path_owned) else {
                    working_tree_change = true;
                    continue;
                };
                // The repo root directory's own entry, with no more specific child path —
                // observed in practice to be reported repeatedly on its own even when
                // nothing inside actually changed (its mtime bumps from `.git`-internal
                // housekeeping alone). Uninformative by itself; real changes always show up
                // as a more specific path elsewhere in the batch, so this contributes to
                // neither signal rather than forcing an unconditional refresh.
                if relative.as_os_str().is_empty() {
                    continue;
                }
                let relevant = match &repo {
                    Some(repo) => is_relevant_change(repo, relative),
                    None => true,
                };
                if !relevant {
                    continue;
                }
                if is_git_internal_path(relative) {
                    git_internal_change = true;
                } else {
                    working_tree_change = true;
                }
            }

            let git_state_really_changed = git_internal_change && {
                let current = git_state_signature(&repo_path_owned);
                let changed = current.is_none() || current != last_git_state;
                last_git_state = current;
                changed
            };

            if working_tree_change || git_state_really_changed {
                on_relevant_change();
            }
        },
    )
    .map_err(|e| PushGitError::Invalid(e.to_string()))?;

    debouncer
        .watch(repo_path, RecursiveMode::Recursive)
        .map_err(|e| PushGitError::Invalid(e.to_string()))?;

    Ok(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    #[test]
    fn dot_git_bookkeeping_paths_are_not_relevant() {
        let (_dir, repo) = repo_init();
        assert!(!is_relevant_change(
            &repo,
            Path::new(".git/objects/ab/cdef1234")
        ));
        assert!(!is_relevant_change(&repo, Path::new(".git/logs/HEAD")));
        assert!(!is_relevant_change(&repo, Path::new(".git/COMMIT_EDITMSG")));
    }

    #[test]
    fn dot_git_ref_state_paths_are_relevant() {
        let (_dir, repo) = repo_init();
        assert!(is_relevant_change(&repo, Path::new(".git/HEAD")));
        assert!(is_relevant_change(&repo, Path::new(".git/index")));
        assert!(is_relevant_change(&repo, Path::new(".git/MERGE_HEAD")));
        assert!(is_relevant_change(&repo, Path::new(".git/refs/heads/main")));
    }

    #[test]
    fn working_tree_paths_are_always_relevant() {
        let (_dir, repo) = repo_init();
        assert!(is_relevant_change(&repo, Path::new("src/main.rs")));
        assert!(is_relevant_change(&repo, Path::new("README.md")));
    }

    #[test]
    fn gitignored_working_tree_paths_are_not_relevant() {
        let (dir, _repo) = repo_init();
        fs::write(dir.path().join(".gitignore"), "target/\n*.log\n").unwrap();
        // Reopened so libgit2 picks up the `.gitignore` just written rather than
        // whatever it may have cached when `_repo` was first opened.
        let repo = Repository::open(dir.path()).unwrap();

        assert!(!is_relevant_change(
            &repo,
            Path::new("target/debug/build-artifact")
        ));
        assert!(!is_relevant_change(&repo, Path::new("app.log")));
        assert!(is_relevant_change(&repo, Path::new("src/main.rs")));
    }

    #[test]
    fn watching_detects_a_working_tree_file_change() {
        let (dir, _repo) = repo_init();
        let seen = Arc::new(Mutex::new(false));
        let seen_writer = Arc::clone(&seen);

        let _debouncer = start_watching(dir.path(), move || {
            *seen_writer.lock().unwrap() = true;
        })
        .unwrap();

        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !*seen.lock().unwrap() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            *seen.lock().unwrap(),
            "expected the watcher to report a relevant change"
        );
    }

    #[test]
    fn git_state_signature_is_stable_when_nothing_changes() {
        let (dir, _repo) = repo_init();
        assert_eq!(
            git_state_signature(dir.path()),
            git_state_signature(dir.path())
        );
    }

    #[test]
    fn git_state_signature_changes_when_head_moves() {
        let (dir, repo) = repo_init();
        let before = git_state_signature(dir.path());

        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add new.txt", &tree, &[&head])
            .unwrap();

        assert_ne!(before, git_state_signature(dir.path()));
    }

    #[test]
    fn rewriting_head_with_identical_bytes_does_not_trigger_a_refresh() {
        let (dir, repo) = repo_init();
        let seen = Arc::new(Mutex::new(false));
        let seen_writer = Arc::clone(&seen);

        let _debouncer = start_watching(dir.path(), move || {
            *seen_writer.lock().unwrap() = true;
        })
        .unwrap();

        // A same-content rewrite: a real mtime bump on a "relevant" `.git` path, but the
        // repo's actual state (what `git_state_signature` captures) hasn't moved at all —
        // exactly the case that used to loop forever.
        let head_path = repo.path().join("HEAD");
        let contents = fs::read(&head_path).unwrap();
        fs::write(&head_path, &contents).unwrap();

        std::thread::sleep(Duration::from_millis(600));

        assert!(
            !*seen.lock().unwrap(),
            "a same-content rewrite of a relevant .git path shouldn't trigger a refresh"
        );
    }

    #[test]
    fn a_real_commit_still_triggers_a_refresh() {
        let (dir, repo) = repo_init();
        let seen = Arc::new(Mutex::new(false));
        let seen_writer = Arc::clone(&seen);

        let _debouncer = start_watching(dir.path(), move || {
            *seen_writer.lock().unwrap() = true;
        })
        .unwrap();

        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add new.txt", &tree, &[&head])
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !*seen.lock().unwrap() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(
            *seen.lock().unwrap(),
            "a real commit moving HEAD/refs should still trigger a refresh"
        );
    }
}
