// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use git2::{Repository, SubmoduleStatus};
use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::PushGitResult;
use crate::remote::{self, RemoteProgress};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub branch: Option<String>,
    pub is_initialized: bool,
    pub is_missing: bool,
    pub is_dirty: bool,
    pub needs_update: bool,
    pub head_id: Option<String>,
    pub workdir_id: Option<String>,
}

pub fn list_submodules(repo: &Repository) -> PushGitResult<Vec<SubmoduleInfo>> {
    let mut result = Vec::new();

    let config = repo.config()?;
    for submodule in repo.submodules()? {
        let Some(name) = submodule.name() else {
            continue;
        };
        let status = repo.submodule_status(name, git2::SubmoduleIgnore::Unspecified)?;
        // IN_CONFIG only means "in .gitmodules"; init is what writes the local url key.
        let is_initialized = config.get_string(&format!("submodule.{name}.url")).is_ok();

        result.push(SubmoduleInfo {
            name: name.to_string(),
            path: submodule.path().display().to_string(),
            url: submodule.url().map(str::to_string),
            branch: submodule.branch().map(str::to_string),
            is_initialized,
            is_missing: !status.is_in_wd(),
            is_dirty: status.contains(SubmoduleStatus::WD_INDEX_MODIFIED)
                || status.is_wd_wd_modified()
                || status.is_wd_untracked(),
            needs_update: status.is_wd_modified(),
            head_id: submodule.head_id().map(|oid| oid.to_string()),
            workdir_id: submodule.workdir_id().map(|oid| oid.to_string()),
        });
    }

    Ok(result)
}

pub fn init_submodule(repo: &Repository, name: &str) -> PushGitResult<()> {
    // Never clobber a URL the user deliberately customized locally, matching git's default.
    repo.find_submodule(name)?.init(false)?;
    Ok(())
}

pub fn sync_submodule(repo: &Repository, name: &str) -> PushGitResult<()> {
    repo.find_submodule(name)?.sync()?;
    Ok(())
}

// Shells out rather than using git2 so SSH/credential helpers behave like the user's own git.
pub async fn update_submodule(
    repo_path: &Path,
    name: Option<&str>,
    recursive: bool,
    progress: &Channel<RemoteProgress>,
    cancel: &Arc<AtomicBool>,
) -> PushGitResult<()> {
    // symlinks=false mitigates CVE-2024-32002; protocol.file.allow is deliberately not overridden.
    let mut args = vec![
        "-c",
        "core.symlinks=false",
        "submodule",
        "update",
        "--init",
        "--progress",
    ];
    if recursive {
        args.push("--recursive");
    }
    if let Some(name) = name {
        args.push("--");
        args.push(name);
    }

    remote::run_git_streaming(&args, Some(repo_path), progress, None, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineOrigin;
    use crate::test_support::{repo_init, set_test_identity};
    use std::fs;
    use std::path::Path as StdPath;
    use tempfile::TempDir;

    fn commit_file(repo: &Repository, name: &str, content: &str) -> git2::Oid {
        fs::write(repo.workdir().unwrap().join(name), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(StdPath::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, name, &tree, &[&head])
            .unwrap()
    }

    fn add_and_commit_submodule(repo: &Repository, url: &str, path: &str) {
        let mut sm = repo.submodule(url, StdPath::new(path), true).unwrap();
        sm.clone(None).unwrap();
        sm.add_to_index(true).unwrap();
        sm.add_finalize().unwrap();
        commit_file(repo, "marker.txt", "x\n");
    }

    // A plain clone never populates submodules, so this is "just cloned, never initialized".
    fn fresh_clone_with_uninitialized_submodule(
        child_url: &str,
        path: &str,
    ) -> (TempDir, Repository) {
        let (upstream_dir, upstream_repo) = repo_init();
        add_and_commit_submodule(&upstream_repo, child_url, path);

        let parent = TempDir::new().unwrap();
        let dest = parent.path().join("clone");
        let repo = Repository::clone(upstream_dir.path().to_str().unwrap(), &dest).unwrap();
        (parent, repo)
    }

    // The submodule is a fresh clone with no identity of its own; CI has no global one either.
    fn commit_inside_submodule(repo: &Repository, path: &str) -> git2::Oid {
        let sub_repo = Repository::open(repo.workdir().unwrap().join(path)).unwrap();
        set_test_identity(&sub_repo);
        commit_file(&sub_repo, "more.txt", "more\n")
    }

    fn no_op_progress() -> (Channel<RemoteProgress>, Arc<AtomicBool>) {
        (Channel::new(|_| Ok(())), Arc::new(AtomicBool::new(false)))
    }

    // Trusted fixtures opt into file-transport clones; the lock spans the await so no test races it.
    #[allow(clippy::await_holding_lock)]
    async fn update_allowing_file_protocol(
        repo_path: &StdPath,
        recursive: bool,
    ) -> PushGitResult<()> {
        let _guard = crate::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("GIT_ALLOW_PROTOCOL");
        std::env::set_var("GIT_ALLOW_PROTOCOL", "file");

        let (progress, cancel) = no_op_progress();
        let result = update_submodule(repo_path, None, recursive, &progress, &cancel).await;

        match previous {
            Some(value) => std::env::set_var("GIT_ALLOW_PROTOCOL", value),
            None => std::env::remove_var("GIT_ALLOW_PROTOCOL"),
        }
        result
    }

    #[test]
    fn list_submodules_reports_a_freshly_added_submodule_as_initialized_and_present() {
        let (_child_dir, child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();

        add_and_commit_submodule(&repo, url, "vendor/lib");

        let list = list_submodules(&repo).unwrap();
        assert_eq!(list.len(), 1);
        let sm = &list[0];
        assert_eq!(sm.path, "vendor/lib");
        assert!(sm.is_initialized);
        assert!(!sm.is_missing);
        assert!(!sm.needs_update);
        assert!(sm.head_id.is_some());
        assert!(sm.workdir_id.is_some());
    }

    #[test]
    fn list_submodules_reports_a_never_cloned_submodule_as_missing_and_uninitialized() {
        let (_child_dir, child_repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let (_clone_dir, repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");

        let list = list_submodules(&repo).unwrap();

        assert_eq!(list.len(), 1);
        assert!(!list[0].is_initialized);
        assert!(list[0].is_missing);
        assert_eq!(list[0].workdir_id, None);
    }

    #[test]
    fn list_submodules_reports_a_dirty_submodule_without_affecting_other_entries() {
        let (_child_dir, child_repo) = repo_init();
        let (_other_child_dir, other_child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let other_url = other_child_repo.workdir().unwrap().to_str().unwrap();

        add_and_commit_submodule(&repo, url, "vendor/dirty");
        add_and_commit_submodule(&repo, other_url, "vendor/clean");

        fs::write(
            repo.workdir().unwrap().join("vendor/dirty/untracked.txt"),
            "x\n",
        )
        .unwrap();

        let list = list_submodules(&repo).unwrap();
        let dirty = list.iter().find(|s| s.path == "vendor/dirty").unwrap();
        let clean = list.iter().find(|s| s.path == "vendor/clean").unwrap();
        assert!(dirty.is_dirty);
        assert!(!clean.is_dirty);
    }

    #[test]
    fn list_submodules_reports_needs_update_when_the_checked_out_commit_differs_from_the_index() {
        let (_child_dir, child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();

        add_and_commit_submodule(&repo, url, "vendor/lib");
        assert!(!list_submodules(&repo).unwrap()[0].needs_update);

        commit_inside_submodule(&repo, "vendor/lib");

        let list = list_submodules(&repo).unwrap();
        assert!(list[0].needs_update);
    }

    #[test]
    fn staging_a_moved_submodule_records_its_new_commit() {
        let (_child_dir, child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        add_and_commit_submodule(&repo, url, "vendor/lib");
        let new_oid = commit_inside_submodule(&repo, "vendor/lib");

        crate::stage::stage_file(&repo, "vendor/lib").unwrap();

        let staged = crate::diff::diff_staged(&repo).unwrap();
        let sub = staged.iter().find(|d| d.is_submodule).unwrap();
        assert!(sub.hunks[0]
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Addition
                && l.content == format!("Subproject commit {new_oid}")));
        assert!(!list_submodules(&repo).unwrap()[0].needs_update);
    }

    #[test]
    fn unstaging_a_moved_submodule_restores_the_head_commit() {
        let (_child_dir, child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        add_and_commit_submodule(&repo, url, "vendor/lib");
        commit_inside_submodule(&repo, "vendor/lib");
        crate::stage::stage_file(&repo, "vendor/lib").unwrap();

        crate::stage::unstage_file(&repo, "vendor/lib").unwrap();

        assert!(crate::diff::diff_staged(&repo).unwrap().is_empty());
        assert!(list_submodules(&repo).unwrap()[0].needs_update);
    }

    #[test]
    fn init_submodule_registers_the_url_in_local_config() {
        let (_child_dir, child_repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let (_clone_dir, repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");
        assert!(!list_submodules(&repo).unwrap()[0].is_initialized);

        init_submodule(&repo, "vendor/lib").unwrap();

        let config = repo.config().unwrap();
        assert_eq!(config.get_string("submodule.vendor/lib.url").unwrap(), url);
        assert!(list_submodules(&repo).unwrap()[0].is_initialized);
    }

    #[test]
    fn init_submodule_leaves_a_customized_local_url_alone() {
        let (_child_dir, child_repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let (_clone_dir, repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");
        let mirror = "https://mirror.invalid/lib.git";
        repo.config()
            .unwrap()
            .set_str("submodule.vendor/lib.url", mirror)
            .unwrap();

        init_submodule(&repo, "vendor/lib").unwrap();

        let config = repo.config().unwrap();
        assert_eq!(
            config.get_string("submodule.vendor/lib.url").unwrap(),
            mirror
        );
    }

    #[test]
    fn sync_submodule_propagates_a_changed_gitmodules_url_into_local_config() {
        let (_child_dir, child_repo) = repo_init();
        let (_other_child_dir, other_child_repo) = repo_init();
        let (_dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let other_url = other_child_repo.workdir().unwrap().to_str().unwrap();

        add_and_commit_submodule(&repo, url, "vendor/lib");
        init_submodule(&repo, "vendor/lib").unwrap();

        let gitmodules_path = repo.workdir().unwrap().join(".gitmodules");
        let contents = fs::read_to_string(&gitmodules_path).unwrap();
        fs::write(&gitmodules_path, contents.replace(url, other_url)).unwrap();
        let repo = Repository::open(repo.workdir().unwrap()).unwrap();

        sync_submodule(&repo, "vendor/lib").unwrap();

        let config = repo.config().unwrap();
        assert_eq!(
            config.get_string("submodule.vendor/lib.url").unwrap(),
            other_url
        );
    }

    #[tokio::test]
    async fn update_submodule_clones_a_missing_submodule_and_checks_out_the_recorded_commit() {
        let (_child_dir, child_repo) = repo_init();
        commit_file(&child_repo, "inside.txt", "inside\n");
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let (_clone_dir, repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");
        assert!(list_submodules(&repo).unwrap()[0].is_missing);
        let repo_path = repo.workdir().unwrap().to_path_buf();

        update_allowing_file_protocol(&repo_path, false)
            .await
            .unwrap();

        assert!(repo_path.join("vendor/lib/inside.txt").exists());
        let info = &list_submodules(&repo).unwrap()[0];
        assert!(!info.is_missing);
        assert!(!info.needs_update);
    }

    #[tokio::test]
    async fn update_submodule_populates_nested_submodules_only_when_recursive() {
        let (_grandchild_dir, grandchild_repo) = repo_init();
        commit_file(&grandchild_repo, "deep.txt", "deep\n");
        let grandchild_url = grandchild_repo.workdir().unwrap().to_str().unwrap();
        let (_child_dir, child_repo) = repo_init();
        add_and_commit_submodule(&child_repo, grandchild_url, "nested");
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        let (_flat_dir, flat_repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");
        let (_deep_dir, deep_repo) = fresh_clone_with_uninitialized_submodule(url, "vendor/lib");
        let flat_path = flat_repo.workdir().unwrap().to_path_buf();
        let deep_path = deep_repo.workdir().unwrap().to_path_buf();

        update_allowing_file_protocol(&flat_path, false)
            .await
            .unwrap();
        update_allowing_file_protocol(&deep_path, true)
            .await
            .unwrap();

        assert!(flat_path.join("vendor/lib/marker.txt").exists());
        assert!(!flat_path.join("vendor/lib/nested/deep.txt").exists());
        assert!(deep_path.join("vendor/lib/nested/deep.txt").exists());
    }

    #[tokio::test]
    async fn update_submodule_is_a_no_op_when_already_up_to_date() {
        let (_child_dir, child_repo) = repo_init();
        let (dir, repo) = repo_init();
        let url = child_repo.workdir().unwrap().to_str().unwrap();
        add_and_commit_submodule(&repo, url, "vendor/lib");

        let (progress, cancel) = no_op_progress();
        update_submodule(dir.path(), None, false, &progress, &cancel)
            .await
            .unwrap();

        assert!(dir.path().join("vendor/lib").join(".git").exists());
    }

    #[tokio::test]
    async fn a_pre_cancelled_token_stops_update_before_it_completes() {
        let (dir, _repo) = repo_init();
        let (progress, _) = no_op_progress();
        let cancel = Arc::new(AtomicBool::new(true));

        let result = update_submodule(dir.path(), None, false, &progress, &cancel).await;

        assert!(matches!(result, Err(crate::error::PushGitError::Cancelled)));
    }
}
