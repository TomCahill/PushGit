// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Filesystem watcher (`notify` crate) that coalesces bursts into batches;
//! filters out `.git/**` noise except the specific paths that matter (`HEAD`, `refs/**`,
//! `index`, `MERGE_HEAD`) and out gitignored working-tree paths (build output, dependency
//! dirs, etc.) so external commits, branch switches, and stashes are detected without
//! triggering on PushGit's own internal writes, a build tool's churn under `target/` or
//! `node_modules/`, or causing refresh loops.
//!
//! One further wrinkle found by hand: the underlying watcher has been observed to
//! re-report the same `.git/HEAD`/`.git/index`/`.git/refs/**` paths as "changed" repeatedly
//! with provably unchanged mtimes — and, worse, to cycle through several distinct
//! full-tree-shaped batches of ordinary working-tree paths, none of which repeats often
//! enough to catch with a simple "same as last batch" check — a self-sustaining loop, since
//! every refresh those trigger reads those same paths right back. Neither
//! [`git_state_signature`] nor [`working_tree_signature`] trusts the watcher's own path
//! list at all: each re-derives the actual state from git (resolved refs/index stat for
//! `.git`-internal paths, `diff_unstaged`'s path/status/insertion/deletion for working-tree
//! ones) and only refreshes when *that* differs from last time.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::OsStr;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use git2::Repository;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};
use serde::Serialize;

use crate::error::{PushGitError, PushGitResult};

const QUIET_PERIOD: Duration = Duration::from_millis(300);
const MAX_BATCH_DELAY: Duration = Duration::from_secs(1);

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
    is_relevant_git_path(git_relative)
}

fn is_relevant_git_path(git_relative: &Path) -> bool {
    let git_relative = git_relative.to_string_lossy().replace('\\', "/");
    git_relative == "HEAD"
        || git_relative == "index"
        || git_relative == "MERGE_HEAD"
        || git_relative.starts_with("refs/")
        || git_relative.starts_with("worktrees/")
        || git_relative.starts_with("modules/")
}

/// A cheap snapshot of exactly the git-internal state `is_relevant_change` cares about:
/// HEAD's resolved target, every ref's target, whether a merge is paused, the index file's
/// mtime/size (its content isn't worth reading here — size+mtime already changes on any real
/// stage/unstage), and the set of linked worktree names — added/removed, not their lock
/// state, which changes rarely and isn't worth an extra `find_worktree`/`is_locked` call per
/// worktree per debounce tick, the same size+mtime-not-content tradeoff already made for the
/// index just above. `None` if the repo can't be opened right now (mid-operation); treated as
/// "unknown, assume changed" by the caller rather than silently swallowing a real change.
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

    let mut worktrees: Vec<String> = repo
        .worktrees()
        .ok()?
        .iter()
        .flatten()
        .map(str::to_string)
        .collect();
    worktrees.sort();

    // A submodule moving to a new commit touches none of the superproject's refs or its index.
    let mut submodules: Vec<String> = repo
        .submodules()
        .ok()?
        .iter()
        .filter_map(|s| {
            let name = s.name()?;
            let workdir_id = s
                .workdir_id()
                .map(|oid| oid.to_string())
                .unwrap_or_default();
            Some(format!("{name}@{workdir_id}"))
        })
        .collect();
    submodules.sort();

    Some(format!(
        "{head_target:?}|{}|{merge_head_present}|{index_stat:?}|{}|{}",
        refs.join(","),
        worktrees.join(","),
        submodules.join(",")
    ))
}

/// Linked worktrees and submodules opened directly keep their git state outside the workdir.
struct RepoPaths {
    workdir: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

#[derive(Debug, PartialEq)]
enum Location<'a> {
    Git(&'a Path),
    WorkingTree(&'a Path),
    Elsewhere,
}

impl RepoPaths {
    fn new(repo: &Repository, workdir: &Path) -> Self {
        // Keeps the caller's spelling of a plain repo's paths; libgit2 canonicalises `repo.path()`.
        if workdir.join(".git").is_dir() {
            return Self::plain(workdir);
        }
        Self {
            workdir: workdir.to_path_buf(),
            git_dir: repo.path().to_path_buf(),
            common_dir: crate::repo::common_dir(repo),
        }
    }

    fn plain(workdir: &Path) -> Self {
        let dot_git = workdir.join(".git");
        Self {
            workdir: workdir.to_path_buf(),
            git_dir: dot_git.clone(),
            common_dir: dot_git,
        }
    }

    fn git_dirs(&self) -> impl Iterator<Item = &Path> {
        let separate_common_dir =
            (self.common_dir != self.git_dir).then_some(self.common_dir.as_path());
        std::iter::once(self.git_dir.as_path()).chain(separate_common_dir)
    }

    // Most specific first: a git dir can sit inside the common dir or the workdir.
    fn locate<'a>(&self, path: &'a Path) -> Location<'a> {
        if let Some(relative) = self.git_dirs().find_map(|dir| path.strip_prefix(dir).ok()) {
            return Location::Git(relative);
        }
        match path.strip_prefix(&self.workdir) {
            Ok(relative) => Location::WorkingTree(relative),
            Err(_) => Location::Elsewhere,
        }
    }

    // The common dir before the git dir, which a linked worktree keeps inside it.
    fn recursive_roots(&self) -> Vec<&Path> {
        let mut roots = vec![self.workdir.as_path()];
        for dir in [&self.common_dir, &self.git_dir] {
            if !roots.iter().any(|root| dir.starts_with(root)) {
                roots.push(dir);
            }
        }
        roots
    }
}

/// Same false-positive-loop concern [`git_state_signature`] guards against for `.git`-internal
/// paths, but observed for ordinary working-tree paths too — and worse there: the underlying
/// watcher was seen cycling through *several* distinct large batches (each looking like a
/// full non-ignored-tree rescan) rather than repeating one fixed set, so a guard that only
/// compares a batch to the single immediately-preceding one doesn't catch it (batch A, B, A,
/// B, ... always looks "different from last"). The robust fix mirrors `git_state_signature`'s
/// approach directly: don't trust the watcher's own path list at all, just ask git what's
/// actually unstaged (path/status/insertion/deletion per file — full hunk content isn't
/// worth reading here, same reasoning `git_state_signature` gives for not hashing the
/// index's content) and only refresh if *that* changed. `None` if the repo can't be opened
/// right now; treated as "unknown, assume changed" by the caller, same as
/// `git_state_signature`.
fn working_tree_signature(repo_path: &Path) -> Option<String> {
    let repo = Repository::open(repo_path).ok()?;
    let diffs = crate::diff::diff_unstaged(&repo).ok()?;
    let mut entries: Vec<String> = diffs
        .iter()
        .map(|d| {
            format!(
                "{}>{}|{:?}|{}|{}|{}",
                d.old_path.as_deref().unwrap_or(""),
                d.new_path.as_deref().unwrap_or(""),
                d.status,
                d.is_binary,
                d.insertions,
                d.deletions,
            )
        })
        .collect();
    entries.sort();
    Some(entries.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub watch_limit_reached: bool,
}

/// Watching stops when this is dropped.
pub struct RepoWatcher {
    stop: Sender<Message>,
    status: WatchStatus,
}

impl RepoWatcher {
    pub fn status(&self) -> WatchStatus {
        self.status
    }
}

impl Drop for RepoWatcher {
    fn drop(&mut self) {
        let _ = self.stop.send(Message::Stop);
    }
}

/// Starts watching `repo_path`, calling `on_relevant_change` (debounced) once
/// per batch containing a relevant working-tree path whose [`working_tree_signature`]
/// actually differs from last time, or a relevant `.git`-internal path whose
/// [`git_state_signature`] actually differs from last time.
/// The returned [`RepoWatcher`] must be kept alive for as long as watching should continue —
/// dropping it stops the watch.
pub fn start_watching(
    repo_path: &Path,
    on_relevant_change: impl FnMut() + Send + 'static,
) -> PushGitResult<RepoWatcher> {
    let workdir = repo_path.to_path_buf();
    let repo = Repository::open(&workdir)?;
    let paths = RepoPaths::new(&repo, &workdir);
    let last_git_state = git_state_signature(&workdir);
    let last_working_tree_state = working_tree_signature(&workdir);

    let (sender, messages) = mpsc::channel();
    let mut watcher =
        RecommendedWatcher::new(forward_changes(sender.clone()), notify::Config::default())
            .map_err(|e| PushGitError::Invalid(e.to_string()))?;

    // inotify spends one watch per directory from a per-user budget; other backends recurse free.
    let (watch_set, watch_limit_reached) = if RecommendedWatcher::kind() == WatcherKind::Inotify {
        let mut watch_set = WatchSet::new(&workdir);
        let complete = watch_set.sync(&mut watcher, &repo);
        (Some(watch_set), !complete)
    } else {
        for root in paths.recursive_roots() {
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|e| PushGitError::Invalid(e.to_string()))?;
        }
        (None, false)
    };

    let session = Session {
        paths,
        watcher,
        watch_set,
        last_git_state,
        last_working_tree_state,
        on_relevant_change,
    };
    std::thread::Builder::new()
        .name("repo-watcher".to_string())
        .spawn(move || session.run(messages))?;

    Ok(RepoWatcher {
        stop: sender,
        status: WatchStatus {
            watch_limit_reached,
        },
    })
}

enum Message {
    Event(Event),
    Stop,
}

fn forward_changes(sender: Sender<Message>) -> impl FnMut(notify::Result<Event>) + Send + 'static {
    move |result| match result {
        // Reads raise these too, so every re-check of the repo would otherwise trigger the next.
        Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
        Ok(event) => {
            let _ = sender.send(Message::Event(event));
        }
        Err(_) => {}
    }
}

/// `None` once the watcher has been stopped.
fn next_batch(messages: &Receiver<Message>) -> Option<Vec<Event>> {
    let Ok(Message::Event(first)) = messages.recv() else {
        return None;
    };
    let mut batch = vec![first];
    let deadline = Instant::now() + MAX_BATCH_DELAY;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Some(batch);
        }
        match messages.recv_timeout(QUIET_PERIOD.min(remaining)) {
            Ok(Message::Event(event)) => batch.push(event),
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return None,
            Err(RecvTimeoutError::Timeout) => return Some(batch),
        }
    }
}

struct Session<F> {
    paths: RepoPaths,
    watcher: RecommendedWatcher,
    watch_set: Option<WatchSet>,
    last_git_state: Option<String>,
    last_working_tree_state: Option<String>,
    on_relevant_change: F,
}

impl<F: FnMut()> Session<F> {
    fn run(mut self, messages: Receiver<Message>) {
        while let Some(events) = next_batch(&messages) {
            self.handle_batch(&events);
        }
    }

    fn handle_batch(&mut self, events: &[Event]) {
        // Re-opened per batch rather than held across the debounce window: cheap
        // (mmap, no object-db scan) and avoids keeping a `Repository` handle alive
        // indefinitely on a thread the caller doesn't control.
        let repo = Repository::open(&self.paths.workdir).ok();

        let rescan = events.iter().any(Event::need_rescan);
        let ignore_rules_changed = events
            .iter()
            .flat_map(|event| &event.paths)
            .any(|path| is_ignore_rules_file(&self.paths, path));

        if let (Some(repo), Some(watch_set)) = (&repo, &mut self.watch_set) {
            watch_set.apply(
                &mut self.watcher,
                repo,
                events,
                rescan || ignore_rules_changed,
            );
        }

        let mut working_tree_change_reported = rescan || ignore_rules_changed;
        let mut git_internal_change = rescan;

        for path in events.iter().flat_map(|event| &event.paths) {
            match self.paths.locate(path) {
                Location::Git(relative) => {
                    if repo.is_none() || is_relevant_git_path(relative) {
                        git_internal_change = true;
                    }
                }
                // The repo root directory's own entry, with no more specific child path —
                // observed in practice to be reported repeatedly on its own even when
                // nothing inside actually changed (its mtime bumps from `.git`-internal
                // housekeeping alone). Uninformative by itself; real changes always show up
                // as a more specific path elsewhere in the batch, so this contributes to
                // neither signal rather than forcing an unconditional refresh.
                Location::WorkingTree(relative) if relative.as_os_str().is_empty() => {}
                Location::WorkingTree(relative) => {
                    let relevant = match &repo {
                        Some(repo) => is_relevant_change(repo, relative),
                        None => true,
                    };
                    if relevant {
                        working_tree_change_reported = true;
                    }
                }
                Location::Elsewhere => working_tree_change_reported = true,
            }
        }

        let working_tree_change = working_tree_change_reported && {
            let current = working_tree_signature(&self.paths.workdir);
            let changed = current.is_none() || current != self.last_working_tree_state;
            self.last_working_tree_state = current;
            changed
        };

        let git_state_really_changed = git_internal_change && {
            let current = git_state_signature(&self.paths.workdir);
            let changed = current.is_none() || current != self.last_git_state;
            self.last_git_state = current;
            changed
        };

        if working_tree_change || git_state_really_changed {
            (self.on_relevant_change)();
        }
    }
}

fn is_ignore_rules_file(paths: &RepoPaths, path: &Path) -> bool {
    match paths.locate(path) {
        Location::Git(relative) => relative == Path::new("info/exclude"),
        Location::WorkingTree(relative) => relative.file_name() == Some(OsStr::new(".gitignore")),
        Location::Elsewhere => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Area {
    WorkingTree,
    GitDir,
    // refs/, worktrees/ and info/ are small, and a relevant file can sit at any depth in them.
    GitSubtree,
    // Path components under modules/ leading to each submodule's own git dir.
    GitModules,
}

fn child_area(repo: &Repository, workdir: &Path, parent: Area, child: &Path) -> Option<Area> {
    let name = child.file_name()?;
    match parent {
        Area::WorkingTree => {
            let relative = child.strip_prefix(workdir).ok()?;
            let skipped = name == ".git" || repo.is_path_ignored(relative).unwrap_or(false);
            (!skipped).then_some(Area::WorkingTree)
        }
        Area::GitDir => match name.to_str()? {
            "refs" | "worktrees" | "info" => Some(Area::GitSubtree),
            "modules" => Some(Area::GitModules),
            _ => None,
        },
        Area::GitSubtree => Some(Area::GitSubtree),
        Area::GitModules if child.join("HEAD").is_file() => Some(Area::GitDir),
        Area::GitModules => Some(Area::GitModules),
    }
}

fn child_dirs(repo: &Repository, workdir: &Path, dir: &Path, area: Area) -> Vec<(PathBuf, Area)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        // Symlinks aren't followed: git doesn't either, and one can point outside the repo.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let child = entry.path();
            child_area(repo, workdir, area, &child).map(|child_area| (child, child_area))
        })
        .collect()
}

/// Breadth-first, finishing each root's tree before starting the next root's.
fn wanted_dirs(
    repo: &Repository,
    workdir: &Path,
    roots: impl IntoIterator<Item = (PathBuf, Area)>,
) -> Vec<(PathBuf, Area)> {
    let mut wanted = Vec::new();
    // A linked worktree's git dir is also under the common dir's worktrees/; the first root wins.
    let mut seen = HashSet::new();
    for root in roots.into_iter().filter(|(dir, _)| dir.is_dir()) {
        let mut queue = VecDeque::from([root]);
        while let Some((dir, area)) = queue.pop_front() {
            if !seen.insert(dir.clone()) {
                continue;
            }
            queue.extend(child_dirs(repo, workdir, &dir, area));
            wanted.push((dir, area));
        }
    }
    wanted
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn removes_path(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(RenameMode::From | RenameMode::Both))
    )
}

fn may_add_directory(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Name(
                RenameMode::To | RenameMode::Both | RenameMode::Any
            ))
    )
}

struct WatchLimitReached;

struct WatchSet {
    workdir: PathBuf,
    watched: BTreeMap<PathBuf, Area>,
}

impl WatchSet {
    fn new(workdir: &Path) -> Self {
        Self {
            workdir: workdir.to_path_buf(),
            watched: BTreeMap::new(),
        }
    }

    /// Returns false if the OS watch limit cut registration short.
    fn sync(&mut self, watcher: &mut dyn Watcher, repo: &Repository) -> bool {
        let paths = RepoPaths::new(repo, &self.workdir);
        // The git dirs go first so HEAD/index/refs changes are still seen if the limit is hit.
        let roots = paths
            .git_dirs()
            .map(|dir| (dir.to_path_buf(), Area::GitDir))
            .chain([(self.workdir.clone(), Area::WorkingTree)]);
        let wanted = wanted_dirs(repo, &self.workdir, roots);
        let wanted_paths: HashSet<&Path> = wanted.iter().map(|(dir, _)| dir.as_path()).collect();
        let unwanted: Vec<PathBuf> = self
            .watched
            .keys()
            .filter(|dir| !wanted_paths.contains(dir.as_path()))
            .cloned()
            .collect();
        for dir in unwanted {
            let _ = watcher.unwatch(&dir);
            self.watched.remove(&dir);
        }

        let mut complete = true;
        for (dir, area) in wanted {
            match self.watched.get_mut(&dir) {
                Some(existing) => *existing = area,
                None if complete => complete = self.watch(watcher, dir, area).is_ok(),
                None => {}
            }
        }
        complete
    }

    fn apply(
        &mut self,
        watcher: &mut dyn Watcher,
        repo: &Repository,
        events: &[Event],
        resync: bool,
    ) {
        // notify's inotify backend already dropped its own watches for these; only the mirror lags.
        for event in events.iter().filter(|event| removes_path(&event.kind)) {
            if let Some(path) = event.paths.first() {
                self.forget_subtree(path);
            }
        }
        if resync {
            self.sync(watcher, repo);
            return;
        }
        for event in events.iter().filter(|event| may_add_directory(&event.kind)) {
            if let Some(path) = event.paths.last() {
                self.follow_new_directory(watcher, repo, path);
            }
        }
    }

    fn follow_new_directory(&mut self, watcher: &mut dyn Watcher, repo: &Repository, dir: &Path) {
        if self.watched.contains_key(dir) || !is_real_dir(dir) {
            return;
        }
        let Some(&parent_area) = dir.parent().and_then(|parent| self.watched.get(parent)) else {
            return;
        };
        let Some(area) = child_area(repo, &self.workdir, parent_area, dir) else {
            return;
        };
        let mut queue = VecDeque::from([(dir.to_path_buf(), area)]);
        while let Some((dir, area)) = queue.pop_front() {
            if self.watched.contains_key(&dir) {
                continue;
            }
            // Watched before listed, so a subdirectory created in between still raises an event.
            if self.watch(watcher, dir.clone(), area).is_err() {
                return;
            }
            queue.extend(child_dirs(repo, &self.workdir, &dir, area));
        }
    }

    fn watch(
        &mut self,
        watcher: &mut dyn Watcher,
        dir: PathBuf,
        area: Area,
    ) -> Result<(), WatchLimitReached> {
        match watcher.watch(&dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                self.watched.insert(dir, area);
                Ok(())
            }
            Err(error) if matches!(error.kind, notify::ErrorKind::MaxFilesWatch) => {
                Err(WatchLimitReached)
            }
            // Already gone again; the parent's watch reports that removal.
            Err(_) => Ok(()),
        }
    }

    fn forget_subtree(&mut self, root: &Path) {
        let forgotten: Vec<PathBuf> = self
            .watched
            .range::<Path, _>((Bound::Included(root), Bound::Unbounded))
            .map(|(dir, _)| dir)
            .take_while(|dir| dir.starts_with(root))
            .cloned()
            .collect();
        for dir in forgotten {
            self.watched.remove(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use notify::event::CreateKind;
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
        assert!(is_relevant_change(
            &repo,
            Path::new(".git/worktrees/feature-wt/HEAD")
        ));
        assert!(is_relevant_change(
            &repo,
            Path::new(".git/modules/vendor/lib/HEAD")
        ));
    }

    #[test]
    fn working_tree_paths_are_always_relevant() {
        let (_dir, repo) = repo_init();
        assert!(is_relevant_change(&repo, Path::new("src/main.rs")));
        assert!(is_relevant_change(&repo, Path::new("README.md")));
    }

    /// Regression test for a self-sustaining refresh loop: the underlying watcher was
    /// observed cycling through several distinct full-tree-shaped batches of working-tree
    /// paths every debounce window indefinitely, with nothing on disk actually changing —
    /// each spurious report used to unconditionally trigger a refresh, storming the app
    /// with reloads fast enough to wipe any in-progress UI state (e.g. checked-but-not-yet-
    /// staged diff lines) before the user could act on it. Comparing the watcher's raw path
    /// list to only the *immediately preceding* batch doesn't catch an A/B/A/B cycle — hence
    /// deriving the signature from git's own idea of what's unstaged instead, exactly like
    /// `git_state_signature` already does for `.git`-internal paths.
    #[test]
    fn working_tree_signature_is_stable_when_nothing_changes() {
        let (dir, _repo) = repo_init();
        assert_eq!(
            working_tree_signature(dir.path()),
            working_tree_signature(dir.path())
        );
    }

    #[test]
    fn working_tree_signature_changes_when_a_file_is_modified() {
        let (dir, _repo) = repo_init();
        let before = working_tree_signature(dir.path());

        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();

        assert_ne!(before, working_tree_signature(dir.path()));
    }

    #[test]
    fn working_tree_signature_is_stable_across_repeated_computation_of_the_same_change() {
        let (dir, _repo) = repo_init();
        fs::write(dir.path().join("new.txt"), "hello\n").unwrap();

        // Simulates the watcher re-deriving the signature on every spurious re-report of an
        // unchanged working tree — it must settle rather than keep "changing" forever.
        assert_eq!(
            working_tree_signature(dir.path()),
            working_tree_signature(dir.path())
        );
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

        let _watcher = start_watching(dir.path(), move || {
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

    /// Adding or removing a worktree that reuses an *existing* branch touches neither
    /// `refs/**` nor the index — without folding the worktree name list into the signature
    /// itself, this class of change would be silently invisible to the watcher despite
    /// `is_relevant_change` now flagging `worktrees/**` paths as relevant.
    #[test]
    fn git_state_signature_changes_when_a_worktree_is_added_or_removed() {
        let (dir, repo) = repo_init();
        crate::branch::create_branch(&repo, "feature", None).unwrap();
        let before = git_state_signature(dir.path());

        let wt_parent = tempfile::TempDir::new().unwrap();
        let wt_path = wt_parent.path().join("feature-wt");
        crate::worktree::add_worktree(&repo, "feature", None, &wt_path).unwrap();
        let after_add = git_state_signature(dir.path());
        assert_ne!(before, after_add);

        crate::worktree::remove_worktree(&repo, "feature-wt").unwrap();
        let after_remove = git_state_signature(dir.path());
        assert_ne!(after_add, after_remove);
        assert_eq!(before, after_remove);
    }

    #[test]
    fn git_state_signature_changes_when_a_submodule_is_updated() {
        let (_child_dir, child_repo) = repo_init();
        let (dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let mut sm = repo.submodule(url, Path::new("sublib"), true).unwrap();
        sm.clone(None).unwrap();
        sm.add_to_index(true).unwrap();
        sm.add_finalize().unwrap();
        let mut index = repo.index().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add submodule", &tree, &[&head])
            .unwrap();

        let before = git_state_signature(dir.path());

        let sub_repo = Repository::open(dir.path().join("sublib")).unwrap();
        crate::test_support::set_test_identity(&sub_repo);
        fs::write(dir.path().join("sublib/more.txt"), "more\n").unwrap();
        let mut sub_index = sub_repo.index().unwrap();
        sub_index.add_path(Path::new("more.txt")).unwrap();
        sub_index.write().unwrap();
        let sub_tree = sub_repo.find_tree(sub_index.write_tree().unwrap()).unwrap();
        let sub_sig = sub_repo.signature().unwrap();
        let sub_head = sub_repo.head().unwrap().peel_to_commit().unwrap();
        sub_repo
            .commit(
                Some("HEAD"),
                &sub_sig,
                &sub_sig,
                "advance",
                &sub_tree,
                &[&sub_head],
            )
            .unwrap();

        assert_ne!(before, git_state_signature(dir.path()));
    }

    #[test]
    fn rewriting_head_with_identical_bytes_does_not_trigger_a_refresh() {
        let (dir, repo) = repo_init();
        let seen = Arc::new(Mutex::new(false));
        let seen_writer = Arc::clone(&seen);

        let _watcher = start_watching(dir.path(), move || {
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

        let _watcher = start_watching(dir.path(), move || {
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

    #[derive(Default)]
    struct FakeWatcher {
        watched: Vec<PathBuf>,
        capacity: Option<usize>,
    }

    impl Watcher for FakeWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            Ok(Self::default())
        }

        fn watch(&mut self, path: &Path, _: RecursiveMode) -> notify::Result<()> {
            if self
                .capacity
                .is_some_and(|capacity| self.watched.len() >= capacity)
            {
                return Err(notify::Error::new(notify::ErrorKind::MaxFilesWatch));
            }
            self.watched.push(path.to_path_buf());
            Ok(())
        }

        fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
            self.watched.retain(|watched| watched != path);
            Ok(())
        }

        fn kind() -> WatcherKind {
            WatcherKind::NullWatcher
        }
    }

    fn relative_to(root: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    fn watched_paths(root: &Path, watcher: &FakeWatcher) -> Vec<String> {
        relative_to(root, &watcher.watched)
    }

    fn synced(root: &Path) -> (WatchSet, FakeWatcher) {
        let repo = Repository::open(root).unwrap();
        let mut watcher = FakeWatcher::default();
        let mut watch_set = WatchSet::new(root);
        assert!(watch_set.sync(&mut watcher, &repo));
        (watch_set, watcher)
    }

    fn folder_created(path: PathBuf) -> Event {
        Event::new(EventKind::Create(CreateKind::Folder)).add_path(path)
    }

    fn wait_for(condition: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        condition()
    }

    fn linked_worktree() -> (tempfile::TempDir, tempfile::TempDir, Repository) {
        let (main_dir, repo) = repo_init();
        crate::branch::create_branch(&repo, "feature", None).unwrap();
        let wt_parent = tempfile::TempDir::new().unwrap();
        let wt_path = wt_parent.path().join("feature-wt");
        crate::worktree::add_worktree(&repo, "feature", None, &wt_path).unwrap();
        let wt_repo = Repository::open(&wt_path).unwrap();
        (main_dir, wt_parent, wt_repo)
    }

    fn watch_for_a_refresh(workdir: &Path) -> (RepoWatcher, Arc<Mutex<bool>>) {
        let seen = Arc::new(Mutex::new(false));
        let seen_writer = Arc::clone(&seen);
        let watcher = start_watching(workdir, move || {
            *seen_writer.lock().unwrap() = true;
        })
        .unwrap();
        (watcher, seen)
    }

    fn commit_nothing_on_head(repo: &Repository) {
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let sig = repo.signature().unwrap();
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "empty",
            &head.tree().unwrap(),
            &[&head],
        )
        .unwrap();
    }

    fn linked_worktree_paths() -> RepoPaths {
        RepoPaths {
            workdir: PathBuf::from("/wt"),
            git_dir: PathBuf::from("/main/.git/worktrees/wt"),
            common_dir: PathBuf::from("/main/.git"),
        }
    }

    // Regression: a recursive watch spent an inotify watch on every node_modules/ directory.
    #[test]
    fn only_unignored_directories_and_relevant_git_internals_are_watched() {
        let (dir, _repo) = repo_init();
        fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();
        fs::create_dir_all(dir.path().join("node_modules/table/node_modules/ajv")).unwrap();
        fs::create_dir_all(dir.path().join("src/components")).unwrap();

        let (_watch_set, watcher) = synced(dir.path());
        let watched = watched_paths(dir.path(), &watcher);

        for expected in ["", "src", "src/components", ".git", ".git/refs/heads"] {
            assert!(
                watched.contains(&expected.to_string()),
                "expected {expected:?} in {watched:?}"
            );
        }
        for skipped in ["node_modules", ".git/objects", ".git/logs"] {
            assert!(
                !watched.iter().any(|path| path.starts_with(skipped)),
                "nothing under {skipped:?} should be watched: {watched:?}"
            );
        }
    }

    #[test]
    fn the_git_dir_is_watched_before_the_working_tree() {
        let (dir, _repo) = repo_init();
        fs::create_dir(dir.path().join("src")).unwrap();

        let (_watch_set, watcher) = synced(dir.path());
        let watched = watched_paths(dir.path(), &watcher);
        let position = |path: &str| watched.iter().position(|w| w == path).unwrap();

        assert!(position(".git/refs/heads") < position(""));
        assert!(position(".git/refs/heads") < position("src"));
    }

    #[test]
    fn running_out_of_watches_keeps_the_ones_already_added() {
        let (dir, repo) = repo_init();
        fs::create_dir(dir.path().join("src")).unwrap();
        let mut watcher = FakeWatcher {
            capacity: Some(2),
            ..FakeWatcher::default()
        };
        let mut watch_set = WatchSet::new(dir.path());

        assert!(!watch_set.sync(&mut watcher, &repo));
        assert_eq!(watched_paths(dir.path(), &watcher)[0], ".git");
        assert_eq!(watch_set.watched.len(), 2);
    }

    #[test]
    fn a_new_directory_is_followed_along_with_anything_already_inside_it() {
        let (dir, repo) = repo_init();
        let (mut watch_set, mut watcher) = synced(dir.path());

        fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        watch_set.apply(
            &mut watcher,
            &repo,
            &[folder_created(dir.path().join("src"))],
            false,
        );

        let watched = watched_paths(dir.path(), &watcher);
        assert!(watched.contains(&"src".to_string()));
        assert!(watched.contains(&"src/nested".to_string()));
    }

    #[test]
    fn a_new_ignored_directory_is_not_followed() {
        let (dir, _repo) = repo_init();
        fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let (mut watch_set, mut watcher) = synced(dir.path());
        let before = watcher.watched.len();

        fs::create_dir_all(dir.path().join("build/out")).unwrap();
        watch_set.apply(
            &mut watcher,
            &repo,
            &[folder_created(dir.path().join("build"))],
            false,
        );

        assert_eq!(watcher.watched.len(), before);
    }

    #[test]
    fn a_renamed_directory_is_followed_under_its_new_name() {
        let (dir, repo) = repo_init();
        fs::create_dir_all(dir.path().join("old/inner")).unwrap();
        let (mut watch_set, mut watcher) = synced(dir.path());

        fs::rename(dir.path().join("old"), dir.path().join("new")).unwrap();
        let renamed = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(dir.path().join("old"))
            .add_path(dir.path().join("new"));
        watch_set.apply(&mut watcher, &repo, &[renamed], false);

        let mirrored: Vec<PathBuf> = watch_set.watched.keys().cloned().collect();
        let mirrored = relative_to(dir.path(), &mirrored);
        assert!(mirrored.contains(&"new/inner".to_string()), "{mirrored:?}");
        assert!(
            !mirrored.iter().any(|path| path.starts_with("old")),
            "{mirrored:?}"
        );
    }

    #[test]
    fn ignore_rule_changes_resync_which_directories_are_watched() {
        let (dir, _repo) = repo_init();
        fs::create_dir(dir.path().join("generated")).unwrap();
        let (mut watch_set, mut watcher) = synced(dir.path());
        let generated = "generated".to_string();
        assert!(watched_paths(dir.path(), &watcher).contains(&generated));

        fs::write(dir.path().join(".gitignore"), "generated/\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        watch_set.apply(&mut watcher, &repo, &[], true);
        assert!(!watched_paths(dir.path(), &watcher).contains(&generated));

        fs::write(dir.path().join(".gitignore"), "").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        watch_set.apply(&mut watcher, &repo, &[], true);
        assert!(watched_paths(dir.path(), &watcher).contains(&generated));
    }

    #[test]
    fn ignore_rule_files_are_recognised_wherever_they_live() {
        let root = &RepoPaths::plain(Path::new("/repo"));
        assert!(is_ignore_rules_file(root, Path::new("/repo/.gitignore")));
        assert!(is_ignore_rules_file(
            root,
            Path::new("/repo/web/.gitignore")
        ));
        assert!(is_ignore_rules_file(
            root,
            Path::new("/repo/.git/info/exclude")
        ));
        assert!(!is_ignore_rules_file(
            root,
            Path::new("/repo/.git/.gitignore")
        ));
        assert!(!is_ignore_rules_file(root, Path::new("/repo/src/main.rs")));
    }

    #[test]
    fn a_submodule_git_dir_under_modules_gets_the_same_allowlist() {
        let (dir, _repo) = repo_init();
        let module = dir.path().join(".git/modules/vendor/lib");
        fs::create_dir_all(module.join("refs/heads")).unwrap();
        fs::create_dir_all(module.join("objects/ab")).unwrap();
        fs::write(module.join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let (_watch_set, watcher) = synced(dir.path());
        let watched = watched_paths(dir.path(), &watcher);

        assert!(watched.contains(&".git/modules/vendor/lib".to_string()));
        assert!(watched.contains(&".git/modules/vendor/lib/refs/heads".to_string()));
        assert!(
            !watched.iter().any(|path| path.contains("objects")),
            "{watched:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_followed() {
        let (dir, _repo) = repo_init();
        let outside = tempfile::TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();

        let (_watch_set, watcher) = synced(dir.path());

        assert!(!watched_paths(dir.path(), &watcher).contains(&"link".to_string()));
    }

    // Regression: each re-check's own reads raised open events that triggered the next one.
    #[test]
    fn reads_are_not_forwarded_as_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "hello\n").unwrap();
        let (sender, messages) = mpsc::channel();
        let mut watcher =
            RecommendedWatcher::new(forward_changes(sender), notify::Config::default()).unwrap();
        watcher
            .watch(dir.path(), RecursiveMode::NonRecursive)
            .unwrap();

        fs::read(dir.path().join("file.txt")).unwrap();
        fs::read_dir(dir.path()).unwrap().for_each(drop);
        assert!(messages.recv_timeout(Duration::from_millis(300)).is_err());

        fs::write(dir.path().join("file.txt"), "changed\n").unwrap();
        assert!(messages.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn edits_inside_a_directory_created_after_watching_started_are_detected() {
        let (dir, repo) = repo_init();
        let refreshes = Arc::new(Mutex::new(0));
        let refreshes_writer = Arc::clone(&refreshes);
        // libgit2's workdir has a trailing slash, same as the path `open_repository` hands out.
        let _watcher = start_watching(repo.workdir().unwrap(), move || {
            *refreshes_writer.lock().unwrap() += 1;
        })
        .unwrap();

        fs::create_dir(dir.path().join("fresh")).unwrap();
        fs::write(dir.path().join("fresh/notes.txt"), "one\n").unwrap();
        assert!(wait_for(|| *refreshes.lock().unwrap() >= 1));

        fs::write(dir.path().join("fresh/notes.txt"), "one\ntwo\n").unwrap();
        assert!(
            wait_for(|| *refreshes.lock().unwrap() >= 2),
            "an edit inside the new directory is only visible through its own watch"
        );
    }

    #[test]
    fn a_linked_worktree_watches_its_git_dir_and_the_common_dirs_refs() {
        let (_main_dir, _wt_parent, wt_repo) = linked_worktree();
        let git_dir = wt_repo.path();
        let common_dir = crate::repo::common_dir(&wt_repo);
        // The git CLI creates this with the worktree; libgit2 doesn't.
        fs::create_dir_all(git_dir.join("logs")).unwrap();

        let (_watch_set, watcher) = synced(wt_repo.workdir().unwrap());
        let position = |dir: &Path| watcher.watched.iter().position(|watched| watched == dir);
        let workdir = position(wt_repo.workdir().unwrap()).expect("the workdir is watched");

        assert!(position(git_dir).is_some_and(|index| index < workdir));
        assert!(position(&common_dir.join("refs/heads")).is_some_and(|index| index < workdir));
        assert_eq!(
            watcher
                .watched
                .iter()
                .filter(|watched| *watched == git_dir)
                .count(),
            1
        );
        for skipped in [common_dir.join("objects"), git_dir.join("logs")] {
            assert!(
                !watcher
                    .watched
                    .iter()
                    .any(|watched| watched.starts_with(&skipped)),
                "nothing under {skipped:?} should be watched: {:?}",
                watcher.watched
            );
        }
    }

    #[test]
    fn a_linked_worktrees_git_state_is_located_in_either_git_dir() {
        let paths = linked_worktree_paths();
        assert_eq!(
            paths.locate(Path::new("/main/.git/worktrees/wt/index")),
            Location::Git(Path::new("index"))
        );
        assert_eq!(
            paths.locate(Path::new("/main/.git/refs/heads/feature")),
            Location::Git(Path::new("refs/heads/feature"))
        );
        assert_eq!(
            paths.locate(Path::new("/wt/src/main.rs")),
            Location::WorkingTree(Path::new("src/main.rs"))
        );
        assert_eq!(
            paths.locate(Path::new("/main/src/main.rs")),
            Location::Elsewhere
        );
    }

    #[test]
    fn a_linked_worktree_takes_ignore_rules_from_its_own_tree_and_the_common_dir() {
        let paths = linked_worktree_paths();
        assert!(is_ignore_rules_file(
            &paths,
            Path::new("/main/.git/info/exclude")
        ));
        assert!(is_ignore_rules_file(&paths, Path::new("/wt/.gitignore")));
        assert!(!is_ignore_rules_file(&paths, Path::new("/main/.gitignore")));
    }

    #[test]
    fn recursive_backends_also_watch_git_dirs_outside_the_workdir() {
        assert_eq!(
            RepoPaths::plain(Path::new("/repo")).recursive_roots(),
            [Path::new("/repo")]
        );
        assert_eq!(
            linked_worktree_paths().recursive_roots(),
            [Path::new("/wt"), Path::new("/main/.git")]
        );
        let submodule = RepoPaths {
            workdir: PathBuf::from("/super/sub"),
            git_dir: PathBuf::from("/super/.git/modules/sub"),
            common_dir: PathBuf::from("/super/.git/modules/sub"),
        };
        assert_eq!(
            submodule.recursive_roots(),
            [
                Path::new("/super/sub"),
                Path::new("/super/.git/modules/sub")
            ]
        );
    }

    #[test]
    fn a_commit_in_a_linked_worktree_triggers_a_refresh() {
        let (_main_dir, _wt_parent, wt_repo) = linked_worktree();
        let (_watcher, seen) = watch_for_a_refresh(wt_repo.workdir().unwrap());

        // Only moves the branch ref, which lives in the main repo's .git, not the worktree.
        commit_nothing_on_head(&wt_repo);

        assert!(
            wait_for(|| *seen.lock().unwrap()),
            "a commit in a linked worktree should trigger a refresh"
        );
    }

    #[test]
    fn staging_in_a_linked_worktree_triggers_a_refresh() {
        let (_main_dir, _wt_parent, wt_repo) = linked_worktree();
        let workdir = wt_repo.workdir().unwrap();
        fs::write(workdir.join("new.txt"), "hello\n").unwrap();
        let (_watcher, seen) = watch_for_a_refresh(workdir);

        // Only rewrites the worktree's index, under the main repo's .git/worktrees/.
        let mut index = wt_repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();

        assert!(
            wait_for(|| *seen.lock().unwrap()),
            "staging in a linked worktree should trigger a refresh"
        );
    }

    #[test]
    fn a_commit_in_a_submodule_opened_directly_triggers_a_refresh() {
        let (_child_dir, child_repo) = repo_init();
        let (dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        repo.submodule(url, Path::new("sublib"), true)
            .unwrap()
            .clone(None)
            .unwrap();
        let sub_repo = Repository::open(dir.path().join("sublib")).unwrap();
        crate::test_support::set_test_identity(&sub_repo);
        assert!(dir.path().join("sublib/.git").is_file());
        let (_watcher, seen) = watch_for_a_refresh(sub_repo.workdir().unwrap());

        commit_nothing_on_head(&sub_repo);

        assert!(
            wait_for(|| *seen.lock().unwrap()),
            "a commit in a submodule opened as its own repo should trigger a refresh"
        );
    }
}
