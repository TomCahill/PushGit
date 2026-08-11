// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-repository/filter graph session: owns the live walk state so `graph_page` resumes
//! exactly where it left off instead of recomputing layout from row 0.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use git2::{BranchType, Oid, Repository, Sort, StatusOptions};
use tokio::sync::Mutex;

use crate::error::{PushGitError, PushGitResult};

use super::color::{BranchKey, ColorAssigner};
use super::color_cache;
use super::commit_graph_file;
use super::lane::{LaneTracker, RowLayout};
use super::model::{
    CommitGraphPage, CommitRow, GraphFilter, RefKind, RefMarker, RowKind, WORKDIR_OID,
};

/// One row in a session's walked/spliced order — either a real commit, or one of the two
/// synthetic kinds this app's stash-on-the-graph addition splices in
/// alongside it. All-`Copy` so `next_page` can iterate `&self.entries[start..end]` and
/// match on `*entry` without cloning.
#[derive(Debug, Clone, Copy)]
enum GraphEntry {
    Commit(Oid),
    /// The working directory's live uncommitted state, anchored to HEAD.
    Workdir {
        parent: Oid,
        file_count: usize,
    },
    /// A real stash commit, anchored to `parent` (`commit.parent_id(0)` — the commit that
    /// was checked out when it was stashed). `index` is its `stash@{N}` list position.
    Stash {
        oid: Oid,
        parent: Oid,
        index: usize,
    },
}

/// The default page size.
pub const DEFAULT_PAGE_SIZE: u32 = 500;
/// The hard cap on page size.
pub const MAX_PAGE_SIZE: u32 = 1000;
/// The idle-eviction window.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

pub struct GraphSession {
    repo: Repository,
    entries: Vec<GraphEntry>,
    /// The number of *real* commits walked — `total_commit_count`'s basis. Tracked
    /// separately from `entries.len()` since that also counts the synthetic workdir/stash
    /// rows spliced in, which shouldn't skew the commit-graph-file-writing threshold.
    commit_count: usize,
    refs_by_oid: HashMap<Oid, Vec<RefMarker>>,
    /// Oids reachable from a local ref (`HEAD`/`refs/heads/*`) — every other oid in `entries`
    /// is only reachable via a tracked branch's ahead upstream. See `CommitRow::is_local`.
    local_oids: HashSet<Oid>,
    lane_tracker: LaneTracker,
    color_assigner: ColorAssigner,
    next_index: usize,
    /// True when `GraphFilter::search` narrowed the walk to an arbitrary subset of history.
    /// Lane/rail continuity isn't meaningful for an arbitrary subset — a matched commit's
    /// parent may itself not match and so never appears in `entries`, which would leave
    /// `LaneTracker` awaiting an oid that's never coming (a lane that never frees, drawing a
    /// rail that never terminates) — so a search result renders as a flat, single-lane list
    /// instead of attempting to preserve branch topology across the gaps. The workdir/stash
    /// splice (`build_entries`) is skipped in this mode for the same reason.
    flat_list: bool,
    /// Refreshed on every `open`/`next_page` call — `GraphSessions::open`'s idle sweep uses
    /// this to evict a session nothing has touched in `IDLE_TIMEOUT`.
    last_accessed: Instant,
}

impl GraphSession {
    pub fn open(repo_path: &Path, filter: &GraphFilter) -> PushGitResult<Self> {
        let mut repo = crate::repo::open(repo_path)?;

        let (local_oids, ordered_oids) = if filter.refs.is_empty() {
            // Local-only walk first, so `is_local` can later tell an ordinary commit apart
            // from one only reachable via a tracked branch's ahead-of-local upstream.
            let mut local_walk = repo.revwalk()?;
            local_walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
            local_walk.push_glob("refs/heads/*")?;
            let _ = local_walk.push_head();
            let local_oids: HashSet<Oid> = local_walk.collect::<Result<HashSet<_>, _>>()?;

            // The real walk: local roots plus, for every local branch with a configured
            // upstream, that upstream's tip — surfacing fetched-but-not-yet-pulled commits
            // (`COMMIT_GRAPH.md`'s remote-tracking color-key rule already anticipated a lane
            // whose current occupant is a remote-only tip, it just never had one to render).
            // A branch with no upstream, or an upstream `git2` can't resolve, contributes
            // nothing — same silent-skip contract `branch::list_branches`'s own ahead/behind
            // computation already uses.
            let mut revwalk = repo.revwalk()?;
            revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
            revwalk.push_glob("refs/heads/*")?;
            let _ = revwalk.push_head();
            for branch in repo.branches(Some(BranchType::Local))? {
                let (branch, _) = branch?;
                if let Ok(upstream) = branch.upstream() {
                    if let Some(target) = upstream.get().target() {
                        let _ = revwalk.push(target);
                    }
                }
            }
            let ordered_oids = revwalk.collect::<Result<Vec<_>, _>>()?;

            (local_oids, ordered_oids)
        } else {
            let mut revwalk = repo.revwalk()?;
            revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
            for name in &filter.refs {
                let oid = repo.revparse_single(name)?.peel_to_commit()?.id();
                revwalk.push(oid)?;
            }
            let ordered_oids = revwalk.collect::<Result<Vec<_>, _>>()?;
            let local_oids: HashSet<Oid> = ordered_oids.iter().copied().collect();
            (local_oids, ordered_oids)
        };

        let refs_by_oid = build_refs_by_oid(&repo)?;
        let commit_count = ordered_oids.len();

        let flat_list = !filter.search.is_empty();
        let entries = if flat_list {
            let query = filter.search.to_lowercase();
            let mut matched = Vec::with_capacity(ordered_oids.len());
            for oid in ordered_oids {
                if commit_matches_search(&repo, oid, refs_by_oid.get(&oid), &query)? {
                    matched.push(GraphEntry::Commit(oid));
                }
            }
            matched
        } else {
            build_entries(&mut repo, ordered_oids, filter)?
        };

        // Seed color assignment from whatever this
        // repo's on-disk cache already knows, so a branch already seen in a past session
        // keeps its color across this app restart too, not just within one session.
        let workdir = repo.workdir().unwrap_or_else(|| repo.path()).to_path_buf();
        let cached_colors = color_cache::load(&workdir);

        Ok(Self {
            repo,
            entries,
            commit_count,
            refs_by_oid,
            local_oids,
            lane_tracker: LaneTracker::new(),
            color_assigner: ColorAssigner::with_cache(
                ColorAssigner::default_persistent_patterns(),
                &cached_colors,
            ),
            next_index: 0,
            flat_list,
            last_accessed: Instant::now(),
        })
    }

    /// The total number of real commits this session walked — used for the
    /// "more than ~5,000 commits" trigger for writing a `commit-graph` file. Already
    /// known for free: `open` collects the full oid list up front regardless.
    pub fn total_commit_count(&self) -> usize {
        self.commit_count
    }

    /// The repository's `.git` directory (`commit_graph_file::exists`'s expected input).
    pub fn git_dir(&self) -> &Path {
        self.repo.path()
    }

    /// The repository's working directory, falling back to the `.git` dir itself for a bare
    /// repo (matching `commands::open_repository`'s own fallback) — `commit_graph_file::
    /// write`'s expected input, since `git commit-graph write` is run via a subprocess whose
    /// cwd must be inside the repo.
    pub fn workdir(&self) -> &Path {
        self.repo.workdir().unwrap_or_else(|| self.repo.path())
    }

    /// Returns the next `page_size` rows, resuming the lane-assignment sweep from wherever
    /// the previous call left off — `O(page_size)` work, never `O(total history)`.
    pub fn next_page(&mut self, page_size: u32) -> PushGitResult<CommitGraphPage> {
        self.last_accessed = Instant::now();
        let page_size = if page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            page_size
        };
        let page_size = (page_size as usize).min(MAX_PAGE_SIZE as usize);
        let start = self.next_index;
        let end = (start + page_size).min(self.entries.len());

        let mut rows = Vec::with_capacity(end.saturating_sub(start));
        let colors_before = self.color_assigner.named_colors().len();

        {
            let repo = &self.repo;
            let refs_by_oid = &self.refs_by_oid;
            let local_oids = &self.local_oids;
            let lane_tracker = &mut self.lane_tracker;
            let color_assigner = &mut self.color_assigner;
            let flat_list = self.flat_list;

            for (offset, entry) in self.entries[start..end].iter().enumerate() {
                let row_index = (start + offset) as u32;

                let row = match *entry {
                    GraphEntry::Commit(oid) => {
                        let commit = repo.find_commit(oid)?;
                        let parent_ids: Vec<Oid> = commit.parent_ids().collect();

                        let layout = if flat_list {
                            RowLayout {
                                lane: 0,
                                color_id: 0,
                                rails: Vec::new(),
                            }
                        } else {
                            lane_tracker.process(
                                oid,
                                &parent_ids,
                                |candidate| {
                                    color_assigner.color_for(branch_key_for(candidate, refs_by_oid))
                                },
                                |first_parent, candidate| {
                                    repo.graph_descendant_of(first_parent, candidate)
                                        .unwrap_or(false)
                                },
                            )
                        };

                        let oid_string = oid.to_string();
                        let short_oid = oid_string.chars().take(7).collect();
                        let author = commit.author();
                        let committer = commit.committer();

                        CommitRow {
                            oid: oid_string,
                            short_oid,
                            row: row_index,
                            summary: commit.summary().unwrap_or_default().to_string(),
                            body: commit.body().map(str::to_string),
                            author_name: author.name().unwrap_or_default().to_string(),
                            author_email: author.email().unwrap_or_default().to_string(),
                            author_time: author.when().seconds(),
                            committer_time: committer.when().seconds(),
                            parents: parent_ids.iter().map(Oid::to_string).collect(),
                            is_merge: parent_ids.len() > 1,
                            is_local: local_oids.contains(&oid),
                            lane: layout.lane,
                            color_id: layout.color_id,
                            refs: refs_by_oid.get(&oid).cloned().unwrap_or_default(),
                            rails: layout.rails,
                            kind: RowKind::Commit,
                            stash_index: None,
                        }
                    }
                    GraphEntry::Workdir { parent, file_count } => {
                        let layout = lane_tracker.process(
                            Oid::zero(),
                            &[parent],
                            |candidate| {
                                color_assigner.color_for(branch_key_for(candidate, refs_by_oid))
                            },
                            |_, _| false,
                        );

                        CommitRow {
                            oid: WORKDIR_OID.to_string(),
                            short_oid: String::new(),
                            row: row_index,
                            summary: workdir_summary(file_count),
                            body: None,
                            author_name: String::new(),
                            author_email: String::new(),
                            author_time: 0,
                            committer_time: 0,
                            parents: vec![parent.to_string()],
                            is_merge: false,
                            is_local: true,
                            lane: layout.lane,
                            color_id: layout.color_id,
                            refs: Vec::new(),
                            rails: layout.rails,
                            kind: RowKind::Workdir,
                            stash_index: None,
                        }
                    }
                    GraphEntry::Stash { oid, parent, index } => {
                        let commit = repo.find_commit(oid)?;
                        let layout = lane_tracker.process(
                            oid,
                            &[parent],
                            |candidate| {
                                color_assigner.color_for(branch_key_for(candidate, refs_by_oid))
                            },
                            |_, _| false,
                        );

                        let oid_string = oid.to_string();
                        let short_oid = oid_string.chars().take(7).collect();
                        let author = commit.author();
                        let committer = commit.committer();

                        CommitRow {
                            oid: oid_string,
                            short_oid,
                            row: row_index,
                            summary: commit.summary().unwrap_or_default().to_string(),
                            body: commit.body().map(str::to_string),
                            author_name: author.name().unwrap_or_default().to_string(),
                            author_email: author.email().unwrap_or_default().to_string(),
                            author_time: author.when().seconds(),
                            committer_time: committer.when().seconds(),
                            parents: vec![parent.to_string()],
                            is_merge: false,
                            is_local: true,
                            lane: layout.lane,
                            color_id: layout.color_id,
                            refs: Vec::new(),
                            rails: layout.rails,
                            kind: RowKind::Stash,
                            stash_index: Some(index),
                        }
                    }
                };

                rows.push(row);
            }
        }

        self.next_index = end;

        // Debounced — one write per page rather
        // than per commit, and skipped entirely once this page didn't actually assign any
        // color this session hadn't already seen (the common case once a repo's branches are
        // fully cached: every subsequent scroll/page-load would otherwise re-write an
        // unchanged file for no reason). `flat_list` mode never assigns real branch colors
        // (every row is color_id 0), so there's nothing meaningful to persist there either.
        if !self.flat_list {
            let named_colors = self.color_assigner.named_colors();
            if named_colors.len() > colors_before {
                let workdir = self.repo.workdir().unwrap_or_else(|| self.repo.path());
                color_cache::save(workdir, &named_colors);
            }
        }

        Ok(CommitGraphPage {
            has_more: end < self.entries.len(),
            rows,
        })
    }
}

/// Resolves the stable `BranchKey` a newly-allocated
/// lane's color should be keyed on — shared by all three `GraphEntry` variants' calls into
/// `LaneTracker::process`.
fn branch_key_for(candidate: Oid, refs_by_oid: &HashMap<Oid, Vec<RefMarker>>) -> BranchKey {
    refs_by_oid
        .get(&candidate)
        .and_then(|refs| {
            refs.iter().find_map(|r| match r.kind {
                RefKind::LocalBranch => Some(BranchKey::Local(r.name.clone())),
                RefKind::RemoteBranch => Some(BranchKey::Remote(r.name.clone())),
                RefKind::Tag => None,
            })
        })
        .unwrap_or_else(|| BranchKey::Synthetic(candidate.to_string()))
}

fn workdir_summary(file_count: usize) -> String {
    if file_count == 1 {
        "Uncommitted changes (1 file)".to_string()
    } else {
        format!("Uncommitted changes ({file_count} files)")
    }
}

/// Splices the live working directory's uncommitted state and any stashes into `ordered_oids`
/// for the "WIP row" and this app's stash-on-the-graph addition. Only done for
/// the unfiltered "whole repo" view: `GraphSession::open`'s `flat_list` (search) mode already
/// renders as an unrelated flat list with no lane continuity to splice into (handled by its
/// own caller, before this is reached), and a `filter.refs`-scoped view (a single branch's
/// history) isn't necessarily anchored at the current working directory's actual parent.
///
/// Each stash is inserted immediately above the row of the commit it was taken from
/// (`commit.parent_id(0)`, ignoring the stash commit's other, git-internal index/untracked
/// parents) — a real child sorts above its parent, so this reproduces exactly where a normal
/// walk would have placed it had it been reachable from a ref. A stash whose base commit
/// isn't in `ordered_oids` at all (e.g. pruned from history, or excluded by a ref filter) is
/// an orphan; there's no correct anchor for it, so it's rendered as a rootless row up top
/// instead of being silently dropped.
fn build_entries(
    repo: &mut Repository,
    ordered_oids: Vec<Oid>,
    filter: &GraphFilter,
) -> PushGitResult<Vec<GraphEntry>> {
    if !filter.refs.is_empty() {
        return Ok(ordered_oids.into_iter().map(GraphEntry::Commit).collect());
    }

    let head_oid = repo.head().ok().and_then(|h| h.target());
    let known: HashSet<Oid> = ordered_oids.iter().copied().collect();

    let mut raw_stashes = Vec::new();
    repo.stash_foreach(|index, _message, &oid| {
        raw_stashes.push((index, oid));
        true
    })?;

    let mut stashes_by_base: HashMap<Oid, Vec<GraphEntry>> = HashMap::new();
    let mut orphaned_stashes = Vec::new();
    for (index, oid) in raw_stashes {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let Ok(parent) = commit.parent_id(0) else {
            continue;
        };
        let entry = GraphEntry::Stash { oid, parent, index };
        if known.contains(&parent) {
            stashes_by_base.entry(parent).or_default().push(entry);
        } else {
            orphaned_stashes.push(entry);
        }
    }

    let mut entries = Vec::with_capacity(ordered_oids.len() + 1 + orphaned_stashes.len());

    if let Some(head) = head_oid {
        if let Some(file_count) = workdir_dirty_file_count(repo)? {
            entries.push(GraphEntry::Workdir {
                parent: head,
                file_count,
            });
        }
    }
    entries.extend(orphaned_stashes);

    for oid in ordered_oids {
        if let Some(stashes) = stashes_by_base.remove(&oid) {
            entries.extend(stashes);
        }
        entries.push(GraphEntry::Commit(oid));
    }

    Ok(entries)
}

/// `Some(count)` of changed paths (staged + unstaged + untracked) if the working directory
/// is dirty, `None` if clean or bare (no working directory at all).
fn workdir_dirty_file_count(repo: &Repository) -> PushGitResult<Option<usize>> {
    if repo.workdir().is_none() {
        return Ok(None);
    }
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut opts))?;
    let count = statuses
        .iter()
        .filter(|entry| !entry.status().is_empty())
        .count();
    Ok((count > 0).then_some(count))
}

/// Whether `oid` matches `query_lower` (already lowercased) per `GraphFilter::search`'s
/// contract: SHA prefix, or a case-insensitive substring of the commit's summary, author
/// name, or any ref pointing at it.
fn commit_matches_search(
    repo: &Repository,
    oid: Oid,
    refs: Option<&Vec<RefMarker>>,
    query_lower: &str,
) -> PushGitResult<bool> {
    if oid.to_string().starts_with(query_lower) {
        return Ok(true);
    }
    if refs.is_some_and(|refs| {
        refs.iter()
            .any(|r| r.name.to_lowercase().contains(query_lower))
    }) {
        return Ok(true);
    }

    let commit = repo.find_commit(oid)?;
    if commit
        .summary()
        .unwrap_or_default()
        .to_lowercase()
        .contains(query_lower)
    {
        return Ok(true);
    }
    let author = commit.author();
    let author_matches = author
        .name()
        .unwrap_or_default()
        .to_lowercase()
        .contains(query_lower);
    Ok(author_matches)
}

/// Builds a lookup from commit oid to every branch/tag ref (and HEAD) pointing at it, so
/// `GraphSession` doesn't re-query the repository's ref state on every row.
fn build_refs_by_oid(repo: &Repository) -> PushGitResult<HashMap<Oid, Vec<RefMarker>>> {
    let mut map: HashMap<Oid, Vec<RefMarker>> = HashMap::new();
    let head_oid = repo.head().ok().and_then(|h| h.target());

    for branch in repo.branches(None)? {
        let (branch, branch_type) = branch?;
        let Some(name) = branch.name()?.map(str::to_string) else {
            continue;
        };
        let Some(target) = branch.get().target() else {
            continue;
        };

        let kind = match branch_type {
            git2::BranchType::Local => RefKind::LocalBranch,
            git2::BranchType::Remote => RefKind::RemoteBranch,
        };
        let is_head = branch_type == git2::BranchType::Local && Some(target) == head_oid;

        map.entry(target).or_default().push(RefMarker {
            name,
            kind,
            is_head,
        });
    }

    for tag_name in repo.tag_names(None)?.iter().flatten() {
        let Ok(reference) = repo.find_reference(&format!("refs/tags/{tag_name}")) else {
            continue;
        };
        let Some(target) = reference.target() else {
            continue;
        };

        // Annotated tags point at a tag object, not the commit directly — peel through it.
        let commit_oid = repo
            .find_object(target, None)
            .ok()
            .and_then(|obj| obj.peel_to_commit().ok())
            .map(|c| c.id())
            .unwrap_or(target);

        map.entry(commit_oid).or_default().push(RefMarker {
            name: tag_name.to_string(),
            kind: RefKind::Tag,
            is_head: false,
        });
    }

    Ok(map)
}

/// Live sessions keyed by an opaque, process-local id. Held in Tauri managed state behind a
/// `tokio::sync::Mutex`, matching `state.rs`'s convention.
#[derive(Default)]
pub struct GraphSessions {
    sessions: Mutex<HashMap<String, GraphSession>>,
    next_id: AtomicU64,
}

impl GraphSessions {
    pub async fn open(&self, repo_path: &Path, filter: &GraphFilter) -> PushGitResult<String> {
        let session = GraphSession::open(repo_path, filter)?;

        // A large repo with no commit-graph file yet gets
        // one written in the background — spawned, never awaited, so it can never delay
        // this call or the graph's first render.
        if commit_graph_file::should_write(
            session.total_commit_count(),
            commit_graph_file::exists(session.git_dir()),
        ) {
            let workdir = session.workdir().to_path_buf();
            tokio::spawn(async move {
                let _ = commit_graph_file::write(&workdir).await;
            });
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let mut sessions = self.sessions.lock().await;
        // The one place this map grows — piggybacking the idle sweep here (rather than a
        // background timer) bounds it to "however many sessions opened in the last
        // `IDLE_TIMEOUT`", with no extra task to manage or cancel on shutdown.
        evict_idle(&mut sessions, Instant::now(), IDLE_TIMEOUT);
        sessions.insert(id.clone(), session);
        Ok(id)
    }

    pub async fn page(&self, session_id: &str, page_size: u32) -> PushGitResult<CommitGraphPage> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| PushGitError::SessionNotFound(session_id.to_string()))?;
        session.next_page(page_size)
    }

    /// Explicit teardown from the frontend's unmount/teardown. The other half of the
    /// session lifecycle — evicting a session whose `graph_close` call never arrives
    /// (e.g. a crashed or reloaded frontend) — is `evict_idle`, run from `open`.
    pub async fn close(&self, session_id: &str) {
        self.sessions.lock().await.remove(session_id);
    }
}

/// Removes every session untouched (no `open`/`next_page` call) for at least
/// `idle_timeout` as of `now`. A free function taking `now` explicitly, rather than
/// calling `Instant::now()` internally, so tests can simulate the timeout elapsing
/// without an actual sleep — `Instant::now() + Duration::from_secs(N)` is a real, valid
/// `Instant`, no mock clock needed.
fn evict_idle(sessions: &mut HashMap<String, GraphSession>, now: Instant, idle_timeout: Duration) {
    sessions.retain(|_, session| now.duration_since(session.last_accessed) < idle_timeout);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::lane::RailKind;
    use crate::test_support::repo_init;

    fn commit_file(
        repo: &Repository,
        name: &str,
        content: &str,
        parents: &[&git2::Commit],
    ) -> git2::Oid {
        commit_file_as(repo, name, content, parents, None)
    }

    /// Like `commit_file`, but with an explicit `(author_name, message)` override instead of
    /// reusing `repo.signature()`/the file name — for tests asserting search matches a
    /// specific author or a message distinct from the changed file's name.
    fn commit_file_as(
        repo: &Repository,
        name: &str,
        content: &str,
        parents: &[&git2::Commit],
        author_and_message: Option<(&str, &str)>,
    ) -> git2::Oid {
        std::fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();

        match author_and_message {
            Some((author_name, message)) => {
                let sig = git2::Signature::now(author_name, "test@example.com").unwrap();
                repo.commit(None, &sig, &sig, message, &tree, parents)
                    .unwrap()
            }
            None => {
                let sig = repo.signature().unwrap();
                repo.commit(None, &sig, &sig, name, &tree, parents).unwrap()
            }
        }
    }

    fn filter_with_search(query: &str) -> GraphFilter {
        GraphFilter {
            refs: Vec::new(),
            search: query.to_string(),
        }
    }

    #[test]
    fn pages_through_a_linear_history() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let second = commit_file(&repo, "a.txt", "a", &[&head]);
        repo.reference("refs/heads/main", second, true, "").ok();
        let second_commit = repo.find_commit(second).unwrap();
        let third = commit_file(&repo, "b.txt", "b", &[&second_commit]);
        repo.reference("refs/heads/main", third, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 3);
        assert!(!page.has_more);
        // Topological + time order puts the newest commit first.
        assert_eq!(page.rows[0].oid, third.to_string());
        assert!(page.rows[0]
            .refs
            .iter()
            .any(|r| r.name == "main" && r.is_head));
    }

    #[test]
    fn a_remote_branch_at_the_same_oid_as_head_is_never_marked_is_head() {
        // `is_head` must require *both* "this is a local branch" and "it targets HEAD's
        // oid" — a remote-tracking ref pointing at that same commit is not the checked-out
        // branch and must never report `is_head: true`, no matter how it targets.
        let (dir, repo) = repo_init();
        let head_oid = repo.head().unwrap().target().unwrap();
        repo.reference("refs/remotes/origin/main", head_oid, true, "")
            .unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        let head_row = page
            .rows
            .iter()
            .find(|r| r.oid == head_oid.to_string())
            .unwrap();
        let remote_ref = head_row
            .refs
            .iter()
            .find(|r| r.name == "origin/main")
            .unwrap();
        assert!(!remote_ref.is_head);
    }

    #[test]
    fn commits_only_reachable_via_an_ahead_upstream_are_marked_not_local() {
        let (dir, repo) = repo_init();
        // Use the repo itself as its own "remote", mirroring
        // `branch::ahead_behind_counts_reflect_divergence_from_upstream`.
        let remote_path = dir.path().to_str().unwrap();
        repo.remote("origin", remote_path).unwrap();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.reference("refs/remotes/origin/main", base.id(), true, "")
            .unwrap();
        let mut branch = repo.find_branch("main", git2::BranchType::Local).unwrap();
        branch.set_upstream(Some("origin/main")).unwrap();

        // Two commits only on the "remote" tracking ref — never on refs/heads/main.
        let ahead_one = repo
            .find_commit(commit_file(&repo, "a.txt", "a", &[&base]))
            .unwrap();
        let ahead_two = commit_file(&repo, "b.txt", "b", &[&ahead_one]);
        repo.reference("refs/remotes/origin/main", ahead_two, true, "")
            .unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        let local_row = page
            .rows
            .iter()
            .find(|r| r.oid == base.id().to_string())
            .unwrap();
        assert!(local_row.is_local);

        let ahead_one_row = page
            .rows
            .iter()
            .find(|r| r.oid == ahead_one.id().to_string())
            .unwrap();
        assert!(!ahead_one_row.is_local);

        let ahead_two_row = page
            .rows
            .iter()
            .find(|r| r.oid == ahead_two.to_string())
            .unwrap();
        assert!(!ahead_two_row.is_local);
        // The lane the two ahead commits sit on must still resolve down to the shared base's
        // lane — no dangling `PassThrough` awaiting an oid the sweep never reaches.
        assert!(ahead_two_row
            .rails
            .iter()
            .any(|r| r.kind == RailKind::ParentEdge));
    }

    #[test]
    fn a_branch_with_no_upstream_configured_contributes_no_extra_roots() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert!(page.rows[0].is_local);
        assert_eq!(page.rows[0].oid, base.id().to_string());
    }

    #[test]
    fn is_merge_is_true_only_for_a_two_parent_commit() {
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        let a = repo
            .find_commit(commit_file(&repo, "a.txt", "a", &[&base]))
            .unwrap();
        let b = repo
            .find_commit(commit_file(&repo, "b.txt", "b", &[&base]))
            .unwrap();
        let merge = commit_file(&repo, "merged", "m", &[&a, &b]);
        repo.reference("refs/heads/main", merge, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        let merge_row = page
            .rows
            .iter()
            .find(|r| r.oid == merge.to_string())
            .unwrap();
        assert!(merge_row.is_merge);
        let single_parent_row = page
            .rows
            .iter()
            .find(|r| r.oid == a.id().to_string())
            .unwrap();
        assert!(!single_parent_row.is_merge);
    }

    #[test]
    fn a_merge_row_does_not_draw_a_spurious_extra_line_for_its_own_new_lane() {
        // Mirrors a real "Merge branch 'origin/develop'" commit: two branches with real,
        // distinct history on both sides. The merge row must draw exactly one rail for the
        // freshly allocated second-parent lane (its `MergeEdge`) — not a `MergeEdge` plus a
        // `PassThrough` for the same lane, which would render as two overlapping green lines
        // at the merge row, one of them a stub with nothing above it to connect to.
        let (dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        let a = repo
            .find_commit(commit_file(&repo, "a.txt", "a", &[&base]))
            .unwrap();
        let b = repo
            .find_commit(commit_file(&repo, "b.txt", "b", &[&base]))
            .unwrap();
        let merge = commit_file(&repo, "merged", "m", &[&a, &b]);
        repo.reference("refs/heads/main", merge, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        let merge_row = page
            .rows
            .iter()
            .find(|r| r.oid == merge.to_string())
            .unwrap();
        let new_lane = merge_row
            .rails
            .iter()
            .find(|r| r.kind == RailKind::MergeEdge)
            .unwrap()
            .to_lane;
        let rails_touching_new_lane = merge_row
            .rails
            .iter()
            .filter(|r| r.from_lane == new_lane || r.to_lane == new_lane)
            .count();
        assert_eq!(rails_touching_new_lane, 1);
    }

    #[test]
    fn a_trivial_merge_does_not_leave_a_persistent_extra_lane() {
        let (dir, repo) = repo_init();
        let root = repo.head().unwrap().peel_to_commit().unwrap();
        let second = repo
            .find_commit(commit_file(&repo, "a.txt", "a", &[&root]))
            .unwrap();
        let third = repo
            .find_commit(commit_file(&repo, "b.txt", "b", &[&second]))
            .unwrap();

        // Merges the repo's own root commit back in as a second parent — already an ancestor
        // of the first parent, so this is a fully trivial/no-op merge with zero unique commits
        // of its own. Before the redundant-merge-parent fix, this second parent would spawn a
        // lane that draws an empty `PassThrough` rail on every row between the merge and the
        // root, several rows down — an uninformative "tail" with no commits ever assigned to it.
        let sig = repo.signature().unwrap();
        let merge = repo
            .commit(
                None,
                &sig,
                &sig,
                "Merge already-contained history",
                &third.tree().unwrap(),
                &[&third, &root],
            )
            .unwrap();
        repo.reference("refs/heads/main", merge, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        let merge_row = page
            .rows
            .iter()
            .find(|r| r.oid == merge.to_string())
            .unwrap();
        assert!(merge_row.is_merge);
        let merge_edge = merge_row
            .rails
            .iter()
            .find(|r| r.kind == RailKind::MergeEdge)
            .unwrap();
        assert_eq!(
            merge_edge.to_lane, merge_row.lane,
            "the redundant merge parent should converge straight back into the merge \
             commit's own lane rather than spawning a new one"
        );

        // Every row between the merge and the root is untouched by a second lane — no
        // `PassThrough` rail should appear anywhere in this page.
        for row in &page.rows {
            assert!(
                row.rails.iter().all(|r| r.kind != RailKind::PassThrough),
                "row {} unexpectedly has a pass-through rail from a phantom lane",
                row.oid
            );
        }
    }

    #[test]
    fn resumes_pagination_without_recomputing_earlier_rows() {
        let (dir, repo) = repo_init();
        let mut parent = repo.head().unwrap().peel_to_commit().unwrap();
        for i in 0..5 {
            let oid = commit_file(&repo, &format!("f{i}.txt"), &format!("{i}"), &[&parent]);
            repo.reference("refs/heads/main", oid, true, "").ok();
            parent = repo.find_commit(oid).unwrap();
        }
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let first_page = session.next_page(3).unwrap();
        let second_page = session.next_page(3).unwrap();

        assert_eq!(first_page.rows.len(), 3);
        assert!(first_page.has_more);
        assert_eq!(second_page.rows.len(), 3); // 6 total commits (initial + 5)
        assert!(!second_page.has_more);
        assert_eq!(first_page.rows[0].row, 0);
        assert_eq!(second_page.rows[0].row, 3);
    }

    #[test]
    fn a_page_that_assigns_a_new_color_persists_it_to_the_on_disk_cache() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let second = commit_file(&repo, "a.txt", "a", &[&head]);
        repo.reference("refs/heads/main", second, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let workdir = repo.workdir().unwrap().to_path_buf();
        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        session.next_page(10).unwrap();

        // `session.rs` always persists against `self.repo.workdir()`, which — unlike the
        // `dir.path()` this fixture's `TempDir` reports — carries a trailing slash; load
        // against the same path `save` used, not a string-inequivalent one.
        let cached = color_cache::load(&workdir);
        assert!(
            cached.contains_key("main"),
            "the current branch's freshly assigned color must be persisted after a page \
             that assigned it, not just kept in memory"
        );
    }

    #[tokio::test]
    async fn unknown_session_id_errors_instead_of_panicking() {
        let sessions = GraphSessions::default();
        let result = sessions.page("does-not-exist", 10).await;
        assert!(matches!(result, Err(PushGitError::SessionNotFound(_))));
    }

    #[tokio::test]
    async fn close_actually_removes_the_session() {
        let (dir, _repo) = repo_init();
        let sessions = GraphSessions::default();
        let id = sessions
            .open(dir.path(), &GraphFilter::default())
            .await
            .unwrap();

        sessions.close(&id).await;

        let result = sessions.page(&id, 10).await;
        assert!(matches!(result, Err(PushGitError::SessionNotFound(_))));
    }

    #[test]
    fn total_commit_count_reflects_every_real_commit_walked() {
        let (dir, repo) = repo_init();
        let mut parent = repo.head().unwrap().peel_to_commit().unwrap();
        for i in 0..4 {
            let oid = commit_file(&repo, &format!("f{i}.txt"), &format!("{i}"), &[&parent]);
            repo.reference("refs/heads/main", oid, true, "").unwrap();
            parent = repo.find_commit(oid).unwrap();
        }

        let session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();

        assert_eq!(session.total_commit_count(), 5); // initial commit + 4 more
    }

    #[test]
    fn evict_idle_keeps_a_session_touched_within_the_timeout() {
        let (dir, _repo) = repo_init();
        let session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let mut sessions = HashMap::new();
        sessions.insert("a".to_string(), session);

        let almost_timed_out = Instant::now() + IDLE_TIMEOUT - Duration::from_secs(1);
        evict_idle(&mut sessions, almost_timed_out, IDLE_TIMEOUT);

        assert!(sessions.contains_key("a"));
    }

    #[test]
    fn evict_idle_removes_a_session_past_the_timeout() {
        let (dir, _repo) = repo_init();
        let session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let mut sessions = HashMap::new();
        sessions.insert("a".to_string(), session);

        let past_timeout = Instant::now() + IDLE_TIMEOUT + Duration::from_secs(1);
        evict_idle(&mut sessions, past_timeout, IDLE_TIMEOUT);

        assert!(sessions.is_empty());
    }

    #[test]
    fn evict_idle_removes_a_session_exactly_at_the_timeout_boundary() {
        let (dir, _repo) = repo_init();
        let session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let last_accessed = session.last_accessed;
        let mut sessions = HashMap::new();
        sessions.insert("a".to_string(), session);

        // Elapsed exactly equal to the timeout counts as timed out (`<`, not `<=`) — a
        // session's grace period is up to but not including the full `IDLE_TIMEOUT`.
        evict_idle(&mut sessions, last_accessed + IDLE_TIMEOUT, IDLE_TIMEOUT);

        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn opening_a_session_evicts_ones_idle_past_the_timeout() {
        let (dir, _repo) = repo_init();
        let sessions = GraphSessions::default();
        let old_id = sessions
            .open(dir.path(), &GraphFilter::default())
            .await
            .unwrap();

        // Backdates the existing session's clock directly rather than sleeping for real
        // 10 minutes — same "pass a real but synthetic `Instant`" approach as `evict_idle`'s
        // own tests above, just reaching through `open`'s public API instead of calling the
        // free function directly.
        {
            let mut guard = sessions.sessions.lock().await;
            guard.get_mut(&old_id).unwrap().last_accessed =
                Instant::now() - IDLE_TIMEOUT - Duration::from_secs(1);
        }

        let new_id = sessions
            .open(dir.path(), &GraphFilter::default())
            .await
            .unwrap();

        let guard = sessions.sessions.lock().await;
        assert!(
            !guard.contains_key(&old_id),
            "idle session should have been evicted"
        );
        assert!(guard.contains_key(&new_id));
    }

    #[test]
    fn a_page_size_of_zero_falls_back_to_the_documented_default() {
        let (dir, _repo) = repo_init();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(0).unwrap();

        // Only one commit exists, so this just confirms the fallback didn't panic or cap to
        // zero rows — the effective page size used is DEFAULT_PAGE_SIZE, not 0.
        assert_eq!(page.rows.len(), 1);
    }

    #[test]
    fn search_matches_a_commit_message_substring() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let first = commit_file_as(
            &repo,
            "a.txt",
            "a",
            &[&head],
            Some(("Ada", "Fix the login bug")),
        );
        repo.reference("refs/heads/main", first, true, "").ok();
        let first_commit = repo.find_commit(first).unwrap();
        let second = commit_file_as(
            &repo,
            "b.txt",
            "b",
            &[&first_commit],
            Some(("Ada", "Add a new feature")),
        );
        repo.reference("refs/heads/main", second, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("login bug")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].summary, "Fix the login bug");
    }

    #[test]
    fn a_multi_paragraph_commit_message_splits_into_summary_and_body() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file_as(
            &repo,
            "a.txt",
            "a",
            &[&head],
            Some((
                "Ada",
                "Fix the login bug\n\nRoot cause was a stale session token.",
            )),
        );
        repo.reference("refs/heads/main", oid, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows[0].summary, "Fix the login bug");
        assert_eq!(
            page.rows[0].body.as_deref(),
            Some("Root cause was a stale session token.")
        );
    }

    #[test]
    fn a_single_line_commit_message_has_no_body() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file_as(
            &repo,
            "a.txt",
            "a",
            &[&head],
            Some(("Ada", "Fix the login bug")),
        );
        repo.reference("refs/heads/main", oid, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows[0].body, None);
    }

    #[test]
    fn search_matches_an_author_name_case_insensitively() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let first = commit_file_as(
            &repo,
            "a.txt",
            "a",
            &[&head],
            Some(("Grace Hopper", "First")),
        );
        repo.reference("refs/heads/main", first, true, "").ok();
        let first_commit = repo.find_commit(first).unwrap();
        let second = commit_file_as(
            &repo,
            "b.txt",
            "b",
            &[&first_commit],
            Some(("Ada Lovelace", "Second")),
        );
        repo.reference("refs/heads/main", second, true, "").unwrap();
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("grace")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].author_name, "Grace Hopper");
    }

    #[test]
    fn search_matches_a_sha_prefix() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file(&repo, "a.txt", "a", &[&head]);
        repo.reference("refs/heads/main", oid, true, "").unwrap();
        let short = &oid.to_string()[..7];

        let mut session = GraphSession::open(dir.path(), &filter_with_search(short)).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].oid, oid.to_string());
    }

    #[test]
    fn search_matches_a_branch_name_pointing_at_the_commit() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file(&repo, "a.txt", "a", &[&head]);
        repo.reference("refs/heads/main", oid, true, "").unwrap();
        repo.branch("release-1.0", &repo.find_commit(oid).unwrap(), false)
            .unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("release-1")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].oid, oid.to_string());
    }

    #[test]
    fn search_results_render_as_a_flat_list_without_lanes_or_rails() {
        let (dir, repo) = repo_init();
        let mut parent = repo.head().unwrap().peel_to_commit().unwrap();
        for i in 0..3 {
            let oid = commit_file_as(
                &repo,
                &format!("f{i}.txt"),
                &format!("{i}"),
                &[&parent],
                Some(("Ada", "matches")),
            );
            repo.reference("refs/heads/main", oid, true, "").ok();
            parent = repo.find_commit(oid).unwrap();
        }
        repo.set_head("refs/heads/main").unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("matches")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 3);
        assert!(page.rows.iter().all(|r| r.lane == 0 && r.rails.is_empty()));
    }

    #[test]
    fn a_blank_search_leaves_the_full_history_and_normal_lanes_untouched() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file(&repo, "a.txt", "a", &[&head]);
        repo.reference("refs/heads/main", oid, true, "").unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 2);
    }

    #[test]
    fn a_clean_working_directory_has_no_workdir_row() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].kind, RowKind::Commit);
        assert_eq!(page.rows[0].oid, head.id().to_string());
    }

    #[test]
    fn a_dirty_working_directory_adds_a_workdir_row_anchored_to_head() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.rows[0].kind, RowKind::Workdir);
        assert_eq!(page.rows[0].oid, WORKDIR_OID);
        assert_eq!(page.rows[0].parents, vec![head.id().to_string()]);
        assert_eq!(page.rows[0].summary, "Uncommitted changes (1 file)");
        assert_eq!(page.rows[1].kind, RowKind::Commit);
        assert_eq!(page.rows[1].oid, head.id().to_string());
        // The workdir row flows straight down into the commit it's parented to.
        assert_eq!(page.rows[0].lane, page.rows[1].lane);
    }

    #[test]
    fn a_stash_taken_from_head_renders_directly_above_it() {
        let (dir, mut repo) = repo_init();
        let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
        std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        let signature = repo.signature().unwrap();
        let stash_oid = repo
            .stash_save(
                &signature,
                "wip test",
                Some(git2::StashFlags::INCLUDE_UNTRACKED),
            )
            .unwrap();

        // `stash_save` cleans the working tree back to HEAD, so no Workdir row competes
        // with the stash row for the top slot.
        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.rows[0].kind, RowKind::Stash);
        assert_eq!(page.rows[0].oid, stash_oid.to_string());
        assert_eq!(page.rows[0].stash_index, Some(0));
        assert_eq!(page.rows[0].parents, vec![head_oid.to_string()]);
        assert_eq!(page.rows[1].kind, RowKind::Commit);
        assert_eq!(page.rows[1].oid, head_oid.to_string());
        // The stash's lane converges into the commit it was taken from, same as a normal
        // branch tip converging on its ancestor.
        assert_eq!(page.rows[0].lane, page.rows[1].lane);
    }

    #[test]
    fn an_orphaned_stash_with_no_reachable_base_still_renders() {
        let (dir, mut repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let head_oid = head.id();

        // A branch whose tip gets stashed from, then deleted — its tip commit becomes
        // unreachable from any ref except the stash's own parent link.
        let side = commit_file(&repo, "side.txt", "s", &[&head]);
        drop(head);
        repo.branch("side", &repo.find_commit(side).unwrap(), false)
            .unwrap();
        repo.set_head("refs/heads/side").unwrap();
        let mut checkout = git2::build::CheckoutBuilder::new();
        repo.checkout_head(Some(checkout.force())).unwrap();

        std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();
        let signature = repo.signature().unwrap();
        let stash_oid = repo
            .stash_save(
                &signature,
                "orphan",
                Some(git2::StashFlags::INCLUDE_UNTRACKED),
            )
            .unwrap();

        repo.set_head("refs/heads/main").unwrap();
        let mut checkout = git2::build::CheckoutBuilder::new();
        repo.checkout_head(Some(checkout.force())).unwrap();
        repo.find_branch("side", git2::BranchType::Local)
            .unwrap()
            .delete()
            .unwrap();

        let mut session = GraphSession::open(dir.path(), &GraphFilter::default()).unwrap();
        let page = session.next_page(10).unwrap();

        // The orphaned stash still renders (as a rootless row up top) alongside the one
        // real commit reachable from `main`.
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.rows[0].kind, RowKind::Stash);
        assert_eq!(page.rows[0].oid, stash_oid.to_string());
        assert_eq!(page.rows[0].parents, vec![side.to_string()]);
        assert_eq!(page.rows[1].kind, RowKind::Commit);
        assert_eq!(page.rows[1].oid, head_oid.to_string());
    }

    #[test]
    fn a_dirty_working_directory_does_not_leak_into_search_results() {
        let (dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let oid = commit_file_as(&repo, "a.txt", "a", &[&head], Some(("Ada", "matches")));
        repo.reference("refs/heads/main", oid, true, "").unwrap();
        std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

        let mut session = GraphSession::open(dir.path(), &filter_with_search("matches")).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].kind, RowKind::Commit);
    }

    #[test]
    fn a_ref_filtered_view_does_not_splice_in_the_workdir_row() {
        let (dir, repo) = repo_init();
        let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();
        repo.reference("refs/heads/main", head_oid, true, "").ok();
        std::fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

        let filter = GraphFilter {
            refs: vec!["main".to_string()],
            search: String::new(),
        };
        let mut session = GraphSession::open(dir.path(), &filter).unwrap();
        let page = session.next_page(10).unwrap();

        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].kind, RowKind::Commit);
    }
}
