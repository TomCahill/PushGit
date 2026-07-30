// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Repo maintenance helper: a one-click `git gc` plus a
//! repo-health indicator (loose object count/size, pack count/size, via `git count-objects
//! -v`) — neither has any git2/libgit2 API at all (confirmed: no `gc`/`repack`/`prune`
//! function anywhere on `git2::Repository`), so both shell out to the real `git` binary,
//! reusing `remote::run_git`'s existing subprocess wrapper rather than duplicating it.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::PushGitResult;
use crate::remote::run_git;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoHealth {
    pub loose_object_count: u64,
    pub loose_object_size_kib: u64,
    pub pack_count: u64,
    pub packed_size_kib: u64,
}

/// A rough repo-health snapshot from `git count-objects -v`'s own key/value report — the
/// same numbers `git gc`'s own "Auto packing the repository" heuristic is based on.
pub async fn repo_health(repo_path: &Path) -> PushGitResult<RepoHealth> {
    let output = run_git(&["count-objects", "-v"], Some(repo_path)).await?;
    Ok(parse_count_objects(&output))
}

fn parse_count_objects(output: &str) -> RepoHealth {
    let values: HashMap<&str, u64> = output
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter_map(|(key, value)| Some((key.trim(), value.trim().parse::<u64>().ok()?)))
        .collect();

    RepoHealth {
        loose_object_count: values.get("count").copied().unwrap_or(0),
        loose_object_size_kib: values.get("size").copied().unwrap_or(0),
        pack_count: values.get("packs").copied().unwrap_or(0),
        packed_size_kib: values.get("size-pack").copied().unwrap_or(0),
    }
}

/// Runs plain `git gc` — no `--aggressive`, matching git's own default speed/safety
/// tradeoff rather than opting into the slower, more thorough repack.
pub async fn run_gc(repo_path: &Path) -> PushGitResult<()> {
    run_git(&["gc"], Some(repo_path)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::repo_init;
    use std::fs;
    use std::path::Path as StdPath;

    #[test]
    fn parses_a_real_count_objects_report() {
        let output = "count: 12\nsize: 48\nin-pack: 100\npacks: 1\nsize-pack: 250\nprune-packable: 0\ngarbage: 0\nsize-garbage: 0\n";

        let health = parse_count_objects(output);

        assert_eq!(health.loose_object_count, 12);
        assert_eq!(health.loose_object_size_kib, 48);
        assert_eq!(health.pack_count, 1);
        assert_eq!(health.packed_size_kib, 250);
    }

    #[test]
    fn parse_count_objects_defaults_missing_fields_to_zero() {
        let health = parse_count_objects("");

        assert_eq!(health.loose_object_count, 0);
        assert_eq!(health.loose_object_size_kib, 0);
        assert_eq!(health.pack_count, 0);
        assert_eq!(health.packed_size_kib, 0);
    }

    #[tokio::test]
    async fn repo_health_reports_real_loose_objects_after_a_commit() {
        let (dir, repo) = repo_init();
        fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(StdPath::new("a.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = repo.signature().unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add a.txt", &tree, &[&head])
            .unwrap();

        let health = repo_health(dir.path()).await.unwrap();

        assert!(health.loose_object_count > 0);
    }

    #[tokio::test]
    async fn run_gc_completes_without_error_on_a_small_repo() {
        let (dir, _repo) = repo_init();

        run_gc(dir.path()).await.unwrap();
    }
}
