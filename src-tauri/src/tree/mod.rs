// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Read-only browsing of a revision's tree or the working tree, for the Browse files view.

mod model;

pub use model::{BlobView, EntryKind, PathEntry, RefStatus, TreeEntry, TreePaths, TreeSource};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use git2::{ErrorCode, ObjectType, Oid, Repository, Status, StatusOptions, Tree, TreeWalkResult};

use crate::diff::{preview_from_bytes, BinaryPreview, FileStatus, MAX_PREVIEW_BYTES};
use crate::error::{PushGitError, PushGitResult};

const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_TREE_PATHS: usize = 200_000;
const FILEMODE_TREE: i32 = 0o040000;
const FILEMODE_LINK: i32 = 0o120000;
const FILEMODE_COMMIT: i32 = 0o160000;
// Same heuristic git itself uses: a NUL in the first 8000 bytes means binary.
const BINARY_SNIFF_BYTES: usize = 8000;

pub fn resolve_rev(repo: &Repository, rev: &str) -> PushGitResult<String> {
    match repo
        .revparse_single(rev)
        .and_then(|obj| obj.peel_to_commit())
    {
        Ok(commit) => Ok(commit.id().to_string()),
        Err(err) if matches!(err.code(), ErrorCode::NotFound | ErrorCode::InvalidSpec) => Err(
            PushGitError::Invalid(format!("No branch, tag or commit called \"{rev}\"")),
        ),
        Err(err) => Err(err.into()),
    }
}

pub fn ref_status(repo: &Repository, rev: &str, pinned_oid: &str) -> PushGitResult<RefStatus> {
    let Ok(current) = resolve_rev(repo, rev) else {
        return Ok(RefStatus::Gone);
    };
    if current == pinned_oid {
        return Ok(RefStatus::Unchanged);
    }
    let (new_oid, old_oid) = (Oid::from_str(&current)?, Oid::from_str(pinned_oid)?);
    let newer_commits = if repo.graph_descendant_of(new_oid, old_oid)? {
        Some(repo.graph_ahead_behind(new_oid, old_oid)?.0)
    } else {
        None
    };
    Ok(RefStatus::Moved {
        oid: current,
        newer_commits,
    })
}

pub fn list_tree(
    repo: &Repository,
    source: &TreeSource,
    dir: &str,
) -> PushGitResult<Vec<TreeEntry>> {
    let mut entries = match source {
        TreeSource::Revision { oid } => list_revision_dir(repo, oid, dir)?,
        TreeSource::WorkingTree => list_workdir_dir(repo, dir)?,
    };
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir, a.name.to_lowercase(), &a.name).cmp(&(
            b.kind != EntryKind::Dir,
            b.name.to_lowercase(),
            &b.name,
        ))
    });
    Ok(entries)
}

pub fn list_tree_paths(repo: &Repository, source: &TreeSource) -> PushGitResult<TreePaths> {
    match source {
        TreeSource::Revision { oid } => revision_paths(repo, oid),
        TreeSource::WorkingTree => workdir_paths(repo),
    }
}

pub fn read_blob_at(
    repo: &Repository,
    source: &TreeSource,
    path: &str,
    want_image: bool,
) -> PushGitResult<BlobView> {
    let rel = validated_relative(path)?;
    match source {
        TreeSource::Revision { oid } => read_revision_blob(repo, oid, &rel, want_image),
        TreeSource::WorkingTree => read_workdir_file(repo, &rel, want_image),
    }
}

fn revision_tree<'r>(repo: &'r Repository, oid: &str) -> PushGitResult<Tree<'r>> {
    Ok(repo.find_commit(Oid::from_str(oid)?)?.tree()?)
}

fn list_revision_dir(repo: &Repository, oid: &str, dir: &str) -> PushGitResult<Vec<TreeEntry>> {
    let root = revision_tree(repo, oid)?;
    let tree = if dir.is_empty() {
        root
    } else {
        root.get_path(&validated_relative(dir)?)?
            .to_object(repo)?
            .peel_to_tree()?
    };
    let odb = repo.odb()?;
    let mut entries = Vec::with_capacity(tree.len());
    for entry in tree.iter() {
        let name = String::from_utf8_lossy(entry.name_bytes()).into_owned();
        let kind = kind_from_filemode(entry.filemode());
        let size = match kind {
            EntryKind::File | EntryKind::Symlink => {
                odb.read_header(entry.id()).ok().map(|(len, _)| len as u64)
            }
            _ => None,
        };
        entries.push(TreeEntry {
            path: join_rel(dir, &name),
            name,
            kind,
            size,
            status: None,
            contains_changes: false,
        });
    }
    Ok(entries)
}

fn kind_from_filemode(mode: i32) -> EntryKind {
    match mode {
        FILEMODE_TREE => EntryKind::Dir,
        FILEMODE_LINK => EntryKind::Symlink,
        FILEMODE_COMMIT => EntryKind::Submodule,
        _ => EntryKind::File,
    }
}

fn revision_paths(repo: &Repository, oid: &str) -> PushGitResult<TreePaths> {
    let tree = revision_tree(repo, oid)?;
    let mut paths = Vec::new();
    let mut truncated = false;
    tree.walk(git2::TreeWalkMode::PreOrder, |parent, entry| {
        if entry.kind() == Some(ObjectType::Tree) {
            return TreeWalkResult::Ok;
        }
        if paths.len() >= MAX_TREE_PATHS {
            truncated = true;
            return TreeWalkResult::Abort;
        }
        let name = String::from_utf8_lossy(entry.name_bytes());
        paths.push(PathEntry {
            path: format!("{parent}{name}"),
            status: None,
        });
        TreeWalkResult::Ok
    })
    .or_else(|err| if truncated { Ok(()) } else { Err(err) })?;
    Ok(TreePaths { paths, truncated })
}

fn read_revision_blob(
    repo: &Repository,
    oid: &str,
    rel: &Path,
    want_image: bool,
) -> PushGitResult<BlobView> {
    let entry = revision_tree(repo, oid)?.get_path(rel)?;
    match entry.filemode() {
        FILEMODE_COMMIT => {
            return Ok(BlobView::Submodule {
                commit: entry.id().to_string(),
            })
        }
        FILEMODE_TREE => return Err(PushGitError::Invalid("That path is a folder".into())),
        _ => {}
    }
    let blob = repo.find_blob(entry.id())?;
    if entry.filemode() == FILEMODE_LINK {
        return Ok(BlobView::Symlink {
            target: String::from_utf8_lossy(blob.content()).into_owned(),
        });
    }
    Ok(view_from_bytes(
        blob.content(),
        blob.size() as u64,
        want_image,
    ))
}

fn view_from_bytes(bytes: &[u8], byte_len: u64, want_image: bool) -> BlobView {
    if want_image {
        return BlobView::Image {
            preview: preview_from_bytes(bytes.to_vec()),
        };
    }
    if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return BlobView::Binary { byte_len };
    }
    let shown = &bytes[..bytes.len().min(MAX_TEXT_BYTES)];
    BlobView::Text {
        content: String::from_utf8_lossy(shown).into_owned(),
        truncated: (shown.len() as u64) < byte_len,
        byte_len,
    }
}

/// Rejects anything but plain relative components so callers can't name a path outside the repo.
fn validated_relative(path: &str) -> PushGitResult<PathBuf> {
    let rel = Path::new(path);
    let mut clean = PathBuf::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) if part != ".git" => clean.push(part),
            _ => return Err(PushGitError::Invalid(format!("Invalid path \"{path}\""))),
        }
    }
    Ok(clean)
}

fn workdir(repo: &Repository) -> PushGitResult<&Path> {
    repo.workdir()
        .ok_or_else(|| PushGitError::Invalid("This repository has no working tree".into()))
}

/// Walks `rel` from the workdir root, refusing to pass through any symlinked directory.
fn confined_workdir_path(repo: &Repository, rel: &Path) -> PushGitResult<PathBuf> {
    let mut full = workdir(repo)?.to_path_buf();
    let parents = rel.parent().map(|p| p.components().count()).unwrap_or(0);
    for (index, component) in rel.components().enumerate() {
        full.push(component);
        if index < parents {
            let meta = fs::symlink_metadata(&full)?;
            if meta.file_type().is_symlink() {
                return Err(PushGitError::Invalid(
                    "Paths through symbolic links aren't followed".into(),
                ));
            }
        }
    }
    Ok(full)
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn working_statuses(repo: &Repository) -> PushGitResult<HashMap<String, FileStatus>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    let mut map = HashMap::new();
    for entry in repo.statuses(Some(&mut opts))?.iter() {
        if let (Some(path), Some(status)) = (entry.path(), map_status(entry.status())) {
            map.insert(path.to_string(), status);
        }
    }
    Ok(map)
}

fn map_status(status: Status) -> Option<FileStatus> {
    Some(if status.is_conflicted() {
        FileStatus::Conflicted
    } else if status.intersects(Status::WT_DELETED | Status::INDEX_DELETED) {
        FileStatus::Deleted
    } else if status.is_wt_new() && !status.is_index_new() {
        FileStatus::Untracked
    } else if status.is_index_new() {
        FileStatus::Added
    } else if status.intersects(Status::WT_RENAMED | Status::INDEX_RENAMED) {
        FileStatus::Renamed
    } else if status.intersects(Status::WT_TYPECHANGE | Status::INDEX_TYPECHANGE) {
        FileStatus::Typechange
    } else if status.intersects(Status::WT_MODIFIED | Status::INDEX_MODIFIED) {
        FileStatus::Modified
    } else {
        return None;
    })
}

fn indexed_paths(repo: &Repository) -> PushGitResult<BTreeMap<String, i32>> {
    let index = repo.index()?;
    Ok(index
        .iter()
        .map(|entry| {
            (
                String::from_utf8_lossy(&entry.path).into_owned(),
                entry.mode as i32,
            )
        })
        .collect())
}

fn list_workdir_dir(repo: &Repository, dir: &str) -> PushGitResult<Vec<TreeEntry>> {
    let rel_dir = validated_relative(dir)?;
    let full_dir = confined_workdir_path(repo, &rel_dir)?;
    if !rel_dir.as_os_str().is_empty() && fs::symlink_metadata(&full_dir)?.file_type().is_symlink()
    {
        return Err(PushGitError::Invalid(
            "Paths through symbolic links aren't followed".into(),
        ));
    }
    let statuses = working_statuses(repo)?;
    let indexed = indexed_paths(repo)?;
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let has_tracked_under = |path: &str| {
        let under = format!("{path}/");
        indexed
            .range(under.clone()..)
            .next()
            .is_some_and(|(p, _)| p.starts_with(&under))
    };
    let has_changes_under = |path: &str| {
        let under = format!("{path}/");
        statuses.keys().any(|p| p.starts_with(&under))
    };

    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for item in fs::read_dir(&full_dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let path = join_rel(dir, &name);
        let meta = item.file_type()?;
        let kind = if meta.is_symlink() {
            EntryKind::Symlink
        } else if meta.is_dir() {
            if indexed.get(&path) == Some(&FILEMODE_COMMIT) {
                EntryKind::Submodule
            } else {
                EntryKind::Dir
            }
        } else {
            EntryKind::File
        };
        let tracked = match kind {
            EntryKind::Dir => has_tracked_under(&path),
            _ => indexed.contains_key(&path),
        };
        if !tracked && repo.is_path_ignored(Path::new(&path)).unwrap_or(false) {
            continue;
        }
        let size = match kind {
            EntryKind::File => item.metadata().ok().map(|m| m.len()),
            _ => None,
        };
        seen.insert(name.clone());
        entries.push(TreeEntry {
            status: if kind == EntryKind::Dir {
                None
            } else {
                statuses.get(&path).copied()
            },
            contains_changes: kind == EntryKind::Dir && has_changes_under(&path),
            name,
            path,
            kind,
            size,
        });
    }

    for (path, status) in &statuses {
        if *status != FileStatus::Deleted {
            continue;
        }
        let Some(rest) = path.strip_prefix(&prefix) else {
            continue;
        };
        let (name, is_dir) = match rest.split_once('/') {
            Some((first, _)) => (first, true),
            None => (rest, false),
        };
        if !seen.insert(name.to_string()) {
            continue;
        }
        let entry_path = join_rel(dir, name);
        entries.push(TreeEntry {
            name: name.to_string(),
            kind: if is_dir {
                EntryKind::Dir
            } else {
                EntryKind::File
            },
            size: None,
            status: if is_dir {
                None
            } else {
                Some(FileStatus::Deleted)
            },
            contains_changes: is_dir,
            path: entry_path,
        });
    }
    Ok(entries)
}

fn workdir_paths(repo: &Repository) -> PushGitResult<TreePaths> {
    let statuses = working_statuses(repo)?;
    let mut all: BTreeMap<String, Option<FileStatus>> = indexed_paths(repo)?
        .into_keys()
        .map(|path| {
            let status = statuses.get(&path).copied();
            (path, status)
        })
        .collect();
    for (path, status) in &statuses {
        all.entry(path.clone()).or_insert(Some(*status));
    }
    let truncated = all.len() > MAX_TREE_PATHS;
    let paths = all
        .into_iter()
        .take(MAX_TREE_PATHS)
        .map(|(path, status)| PathEntry { path, status })
        .collect();
    Ok(TreePaths { paths, truncated })
}

fn read_workdir_file(repo: &Repository, rel: &Path, want_image: bool) -> PushGitResult<BlobView> {
    let rel_str = rel.to_string_lossy();
    if let Some(entry) = repo.index()?.get_path(rel, 0) {
        if entry.mode as i32 == FILEMODE_COMMIT {
            return Ok(BlobView::Submodule {
                commit: entry.id.to_string(),
            });
        }
    }
    let full = confined_workdir_path(repo, rel)?;
    let meta = match fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BlobView::Deleted),
        Err(err) => return Err(err.into()),
    };
    if meta.file_type().is_symlink() {
        return Ok(BlobView::Symlink {
            target: fs::read_link(&full)?.to_string_lossy().into_owned(),
        });
    }
    if meta.is_dir() {
        return Err(PushGitError::Invalid(format!("\"{rel_str}\" is a folder")));
    }
    let byte_len = meta.len();
    if want_image && byte_len > MAX_PREVIEW_BYTES as u64 {
        return Ok(BlobView::Image {
            preview: BinaryPreview::TooLarge { byte_len },
        });
    }
    let limit = if want_image {
        byte_len
    } else {
        MAX_TEXT_BYTES as u64
    };
    let mut bytes = Vec::new();
    fs::File::open(&full)?.take(limit).read_to_end(&mut bytes)?;
    Ok(view_from_bytes(&bytes, byte_len, want_image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;

    fn write(repo: &Repository, path: &str, content: &str) {
        let full = repo.workdir().unwrap().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }

    fn commit_all(repo: &Repository, message: &str) -> Oid {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&head])
            .unwrap()
    }

    fn names(entries: &[TreeEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn rev(oid: Oid) -> TreeSource {
        TreeSource::Revision {
            oid: oid.to_string(),
        }
    }

    #[test]
    fn revision_listing_puts_folders_first_and_reads_old_content() {
        let (_dir, repo) = repo_init();
        write(&repo, "b.txt", "old\n");
        write(&repo, "src/a.rs", "fn a() {}\n");
        let first = commit_all(&repo, "first");
        write(&repo, "b.txt", "new\n");
        commit_all(&repo, "second");

        let root = list_tree(&repo, &rev(first), "").unwrap();
        assert_eq!(names(&root), vec!["src", "b.txt"]);
        assert_eq!(root[0].kind, EntryKind::Dir);
        assert_eq!(root[1].size, Some(4));
        assert_eq!(
            names(&list_tree(&repo, &rev(first), "src").unwrap()),
            vec!["a.rs"]
        );

        match read_blob_at(&repo, &rev(first), "b.txt", false).unwrap() {
            BlobView::Text {
                content, truncated, ..
            } => {
                assert_eq!(content, "old\n");
                assert!(!truncated);
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn binary_and_truncated_blobs_are_reported() {
        let (_dir, repo) = repo_init();
        fs::write(repo.workdir().unwrap().join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        write(&repo, "big.txt", &"x".repeat(MAX_TEXT_BYTES + 10));
        let oid = commit_all(&repo, "files");

        assert!(matches!(
            read_blob_at(&repo, &rev(oid), "bin.dat", false).unwrap(),
            BlobView::Binary { byte_len: 4 }
        ));
        match read_blob_at(&repo, &rev(oid), "big.txt", false).unwrap() {
            BlobView::Text {
                content,
                truncated,
                byte_len,
            } => {
                assert!(truncated);
                assert_eq!(content.len(), MAX_TEXT_BYTES);
                assert_eq!(byte_len, (MAX_TEXT_BYTES + 10) as u64);
            }
            other => panic!("expected text, got {other:?}"),
        }
        assert!(matches!(
            read_blob_at(&repo, &rev(oid), "bin.dat", true).unwrap(),
            BlobView::Image { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn committed_symlink_shows_its_target_not_the_file_it_points_at() {
        let (_dir, repo) = repo_init();
        let outside = tempfile::TempDir::new().unwrap();
        let secret = outside.path().join("secret");
        fs::write(&secret, "do not show").unwrap();
        std::os::unix::fs::symlink(&secret, repo.workdir().unwrap().join("link")).unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.workdir().unwrap().join("linkdir"))
            .unwrap();
        let oid = commit_all(&repo, "links");

        for source in [rev(oid), TreeSource::WorkingTree] {
            match read_blob_at(&repo, &source, "link", false).unwrap() {
                BlobView::Symlink { target } => assert_eq!(target, secret.to_string_lossy()),
                other => panic!("expected symlink, got {other:?}"),
            }
        }
        let root = list_tree(&repo, &TreeSource::WorkingTree, "").unwrap();
        assert!(root.iter().all(|e| e.kind == EntryKind::Symlink));
        assert!(read_blob_at(&repo, &TreeSource::WorkingTree, "linkdir/secret", false).is_err());
        assert!(list_tree(&repo, &TreeSource::WorkingTree, "linkdir").is_err());
    }

    #[test]
    fn paths_that_escape_the_repo_are_rejected() {
        let (_dir, repo) = repo_init();
        for bad in ["../x", "/etc/passwd", "a/../../x", ".git/config"] {
            assert!(
                read_blob_at(&repo, &TreeSource::WorkingTree, bad, false).is_err(),
                "{bad}"
            );
            assert!(
                list_tree(&repo, &TreeSource::WorkingTree, bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn working_tree_hides_ignored_and_shows_status_including_deleted() {
        let (_dir, repo) = repo_init();
        write(&repo, ".gitignore", "target/\n*.log\n");
        write(&repo, "src/lib.rs", "one\n");
        write(&repo, "src/old.rs", "old\n");
        write(&repo, "tracked.log", "kept\n");
        commit_all(&repo, "base");
        write(&repo, "src/lib.rs", "two\n");
        fs::remove_file(repo.workdir().unwrap().join("src/old.rs")).unwrap();
        write(&repo, "src/new.rs", "new\n");
        write(&repo, "target/out.bin", "build\n");
        write(&repo, "debug.log", "noise\n");
        // `add_all` respects .gitignore, so force-track the ignored-but-tracked log.
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.log")).unwrap();
        index.write().unwrap();

        let root = list_tree(&repo, &TreeSource::WorkingTree, "").unwrap();
        assert_eq!(names(&root), vec!["src", ".gitignore", "tracked.log"]);
        assert!(root[0].contains_changes);

        let src = list_tree(&repo, &TreeSource::WorkingTree, "src").unwrap();
        let status_of = |name: &str| src.iter().find(|e| e.name == name).unwrap().status;
        assert_eq!(status_of("lib.rs"), Some(FileStatus::Modified));
        assert_eq!(status_of("new.rs"), Some(FileStatus::Untracked));
        assert_eq!(status_of("old.rs"), Some(FileStatus::Deleted));

        assert!(matches!(
            read_blob_at(&repo, &TreeSource::WorkingTree, "src/old.rs", false).unwrap(),
            BlobView::Deleted
        ));

        let paths = list_tree_paths(&repo, &TreeSource::WorkingTree).unwrap();
        let listed: Vec<&str> = paths.paths.iter().map(|p| p.path.as_str()).collect();
        assert!(listed.contains(&"src/new.rs"));
        assert!(listed.contains(&"src/old.rs"));
        assert!(!listed
            .iter()
            .any(|p| p.starts_with("target/") || *p == "debug.log"));
    }

    #[test]
    fn revision_paths_lists_every_file() {
        let (_dir, repo) = repo_init();
        write(&repo, "a.txt", "a");
        write(&repo, "d/e/f.txt", "f");
        let oid = commit_all(&repo, "files");
        let paths = list_tree_paths(&repo, &rev(oid)).unwrap();
        let listed: Vec<&str> = paths.paths.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(listed, vec!["a.txt", "d/e/f.txt"]);
        assert!(!paths.truncated);
    }

    #[test]
    fn resolve_rev_peels_annotated_tags_and_names_bad_refs() {
        let (_dir, repo) = repo_init();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        let sig = repo.signature().unwrap();
        repo.tag("v1", head.as_object(), &sig, "release", false)
            .unwrap();

        assert_eq!(resolve_rev(&repo, "v1").unwrap(), head.id().to_string());
        let err = resolve_rev(&repo, "nope").unwrap_err().to_string();
        assert_eq!(err, "No branch, tag or commit called \"nope\"");
    }

    #[test]
    fn ref_status_counts_new_commits_or_reports_a_rewrite() {
        let (_dir, repo) = repo_init();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        write(&repo, "a.txt", "a");
        let ahead = commit_all(&repo, "ahead");

        assert!(matches!(
            ref_status(&repo, "main", &ahead.to_string()).unwrap(),
            RefStatus::Unchanged
        ));
        assert!(matches!(
            ref_status(&repo, "main", &base.id().to_string()).unwrap(),
            RefStatus::Moved {
                newer_commits: Some(1),
                ..
            }
        ));

        repo.branch("side", &base, false).unwrap();
        let sig = repo.signature().unwrap();
        let tree = base.tree().unwrap();
        let rewritten = repo
            .commit(None, &sig, &sig, "rewritten", &tree, &[&base])
            .unwrap();
        repo.reference("refs/heads/side", rewritten, true, "test")
            .unwrap();
        assert!(matches!(
            ref_status(&repo, "side", &ahead.to_string()).unwrap(),
            RefStatus::Moved {
                newer_commits: None,
                ..
            }
        ));
        assert!(matches!(
            ref_status(&repo, "deleted-branch", &ahead.to_string()).unwrap(),
            RefStatus::Gone
        ));
    }
}
