// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! GitFlow workflow detection/config and start/finish operations. Built entirely on top of
//! `branch::create_branch`/`checkout_branch`/`merge_branch`/`create_tag`/`delete_branch` — no
//! new git mutation primitives, only GitFlow's naming/base-branch/tag conventions layered
//! over them. Reads and writes the same `.git/config` `[gitflow ...]` keys the `git-flow`
//! (AVH edition) CLI uses, so a repo set up with either tool works with the other.

mod model;

pub use model::{FinishOutcome, WorkflowBranchKind, WorkflowConfig};

use git2::{BranchType, Repository};

use crate::branch::{self, MergeOutcome};
use crate::error::PushGitResult;

/// Reads `gitflow.branch.master`/`gitflow.branch.develop`/`gitflow.prefix.*` from
/// `.git/config`. `None` means "not initialized" (either or both of the two branch keys
/// missing) — distinct from an error, since an un-configured repo is the expected, common
/// case, not a failure.
pub fn detect_workflow(repo: &Repository) -> PushGitResult<Option<WorkflowConfig>> {
    let config = repo.config()?;

    let Some(main) = config.get_string("gitflow.branch.master").ok() else {
        return Ok(None);
    };
    let Some(develop) = config.get_string("gitflow.branch.develop").ok() else {
        return Ok(None);
    };

    Ok(Some(WorkflowConfig {
        main,
        develop,
        feature_prefix: config
            .get_string("gitflow.prefix.feature")
            .unwrap_or_default(),
        release_prefix: config
            .get_string("gitflow.prefix.release")
            .unwrap_or_default(),
        hotfix_prefix: config
            .get_string("gitflow.prefix.hotfix")
            .unwrap_or_default(),
        support_prefix: config.get_string("gitflow.prefix.support").ok(),
        version_tag_prefix: config
            .get_string("gitflow.prefix.versiontag")
            .unwrap_or_default(),
    }))
}

/// Writes `config`'s fields as `gitflow.branch.*`/`gitflow.prefix.*` (equivalent to
/// `git flow init`), and creates the `develop` branch from `main`'s current tip if it doesn't
/// already exist — matching real `git flow init`'s behavior so a fresh repo doesn't need a
/// manual branch-creation step first.
pub fn init_workflow(repo: &Repository, config: &WorkflowConfig) -> PushGitResult<()> {
    {
        let mut git_config = repo.config()?;
        git_config.set_str("gitflow.branch.master", &config.main)?;
        git_config.set_str("gitflow.branch.develop", &config.develop)?;
        git_config.set_str("gitflow.prefix.feature", &config.feature_prefix)?;
        git_config.set_str("gitflow.prefix.release", &config.release_prefix)?;
        git_config.set_str("gitflow.prefix.hotfix", &config.hotfix_prefix)?;
        git_config.set_str("gitflow.prefix.versiontag", &config.version_tag_prefix)?;
        if let Some(support) = &config.support_prefix {
            git_config.set_str("gitflow.prefix.support", support)?;
        }
    }

    if repo
        .find_branch(&config.develop, BranchType::Local)
        .is_err()
    {
        let main_branch = repo.find_branch(&config.main, BranchType::Local)?;
        let target = main_branch.get().peel_to_commit()?;
        repo.branch(&config.develop, &target, false)?;
    }

    Ok(())
}

fn prefix_for(config: &WorkflowConfig, kind: WorkflowBranchKind) -> &str {
    match kind {
        WorkflowBranchKind::Feature => &config.feature_prefix,
        WorkflowBranchKind::Release => &config.release_prefix,
        WorkflowBranchKind::Hotfix => &config.hotfix_prefix,
    }
}

fn base_branch_for(config: &WorkflowConfig, kind: WorkflowBranchKind) -> &str {
    match kind {
        WorkflowBranchKind::Feature | WorkflowBranchKind::Release => &config.develop,
        WorkflowBranchKind::Hotfix => &config.main,
    }
}

/// Creates `<prefix><name>` off the correct base branch for `kind` and checks it out. Thin
/// wrapper over `branch::create_branch` + `branch::checkout_branch`; the only new logic here
/// is picking the prefix and base branch from `config`.
pub fn start_branch(
    repo: &Repository,
    config: &WorkflowConfig,
    kind: WorkflowBranchKind,
    name: &str,
) -> PushGitResult<()> {
    let branch_name = format!("{}{name}", prefix_for(config, kind));
    let base = base_branch_for(config, kind);
    branch::create_branch(repo, &branch_name, Some(base))?;
    branch::checkout_branch(repo, &branch_name)?;
    Ok(())
}

/// Merges `<prefix><name>` into its finish target(s) and deletes it:
/// - **feature**: merge into `develop` (fast-forwarding when possible), delete.
/// - **release**/**hotfix**: merge into `main`, tag `main` at that point with
///   `version_tag_prefix + name`, merge into `develop` too, then delete. These two merges
///   always produce a real merge commit (`merge_branch_no_ff`), even when a fast-forward
///   would be possible, so the graph always shows the release/hotfix branch as a visible
///   fork+merge rather than silently collapsing into the target's history.
///
/// If any merge step conflicts, returns early with `FinishOutcome::Conflicts` and leaves the
/// branch and remaining steps untouched — the caller resolves via the existing
/// conflict-resolution UI, commits, and calls `finish_branch` again with the same arguments.
/// This is safe because every step here is idempotent on replay (a merge whose source is
/// already an ancestor of the target is a no-op `AlreadyUpToDate`, tag creation is skipped if
/// the tag already exists, and branch deletion is skipped if the branch is already gone) —
/// there's no separate "continue" command or persisted "which step" state to get out of sync.
pub fn finish_branch(
    repo: &Repository,
    config: &WorkflowConfig,
    kind: WorkflowBranchKind,
    name: &str,
) -> PushGitResult<FinishOutcome> {
    let branch_name = format!("{}{name}", prefix_for(config, kind));
    let is_release_or_hotfix = matches!(
        kind,
        WorkflowBranchKind::Release | WorkflowBranchKind::Hotfix
    );

    if is_release_or_hotfix {
        branch::checkout_branch(repo, &config.main)?;
        if let MergeOutcome::Conflicts(paths) = branch::merge_branch_no_ff(repo, &branch_name)? {
            return Ok(FinishOutcome::Conflicts(paths));
        }
        create_tag_if_absent(repo, config, name)?;
    }

    branch::checkout_branch(repo, &config.develop)?;
    let develop_outcome = if is_release_or_hotfix {
        branch::merge_branch_no_ff(repo, &branch_name)?
    } else {
        branch::merge_branch(repo, &branch_name)?
    };
    if let MergeOutcome::Conflicts(paths) = develop_outcome {
        return Ok(FinishOutcome::Conflicts(paths));
    }

    delete_branch_if_present(repo, &branch_name)?;

    Ok(FinishOutcome::Finished)
}

fn create_tag_if_absent(
    repo: &Repository,
    config: &WorkflowConfig,
    name: &str,
) -> PushGitResult<()> {
    let tag_name = format!("{}{name}", config.version_tag_prefix);
    if branch::list_tags(repo)?.iter().any(|t| t == &tag_name) {
        return Ok(());
    }
    branch::create_tag(repo, &tag_name, None, None)
}

fn delete_branch_if_present(repo: &Repository, name: &str) -> PushGitResult<()> {
    if repo.find_branch(name, BranchType::Local).is_ok() {
        branch::delete_branch(repo, name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage;
    use crate::test_support::repo_init;
    use std::fs;
    use std::path::Path;

    fn commit_file(repo: &Repository, name: &str, content: &str) -> git2::Oid {
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

    fn init_default(repo: &Repository) -> WorkflowConfig {
        let config = WorkflowConfig::default();
        init_workflow(repo, &config).unwrap();
        config
    }

    #[test]
    fn detect_workflow_is_none_before_init() {
        let (_dir, repo) = repo_init();
        assert_eq!(detect_workflow(&repo).unwrap(), None);
    }

    #[test]
    fn init_workflow_writes_config_and_creates_develop() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);

        assert!(repo.find_branch("develop", BranchType::Local).is_ok());
        assert_eq!(detect_workflow(&repo).unwrap(), Some(config));
    }

    #[test]
    fn init_workflow_does_not_recreate_an_existing_develop_branch() {
        let (_dir, repo) = repo_init();
        branch::create_branch(&repo, "develop", None).unwrap();
        branch::checkout_branch(&repo, "develop").unwrap();
        let develop_tip = commit_file(&repo, "on-develop.txt", "x\n");
        branch::checkout_branch(&repo, "main").unwrap();

        init_default(&repo);

        let develop = repo.find_branch("develop", BranchType::Local).unwrap();
        assert_eq!(develop.get().target().unwrap(), develop_tip);
    }

    #[test]
    fn start_branch_feature_branches_off_develop_and_checks_it_out() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);

        start_branch(&repo, &config, WorkflowBranchKind::Feature, "widget").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("feature/widget"));
        let branch = repo
            .find_branch("feature/widget", BranchType::Local)
            .unwrap();
        let develop = repo.find_branch("develop", BranchType::Local).unwrap();
        assert_eq!(branch.get().target(), develop.get().target());
    }

    #[test]
    fn start_branch_hotfix_branches_off_main() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);
        branch::checkout_branch(&repo, "develop").unwrap();
        commit_file(&repo, "develop-only.txt", "x\n");

        start_branch(&repo, &config, WorkflowBranchKind::Hotfix, "1.0.1").unwrap();

        assert_eq!(repo.head().unwrap().shorthand(), Some("hotfix/1.0.1"));
        let branch = repo.find_branch("hotfix/1.0.1", BranchType::Local).unwrap();
        let main = repo.find_branch("main", BranchType::Local).unwrap();
        assert_eq!(branch.get().target(), main.get().target());
    }

    #[test]
    fn finish_branch_feature_merges_into_develop_and_deletes_it() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);
        start_branch(&repo, &config, WorkflowBranchKind::Feature, "widget").unwrap();
        commit_file(&repo, "widget.txt", "widget\n");

        let outcome = finish_branch(&repo, &config, WorkflowBranchKind::Feature, "widget").unwrap();

        assert!(matches!(outcome, FinishOutcome::Finished));
        assert!(repo
            .find_branch("feature/widget", BranchType::Local)
            .is_err());
        let develop = repo.find_branch("develop", BranchType::Local).unwrap();
        let develop_tree = repo
            .find_commit(develop.get().target().unwrap())
            .unwrap()
            .tree()
            .unwrap();
        assert!(develop_tree.get_path(Path::new("widget.txt")).is_ok());
    }

    #[test]
    fn finish_branch_release_tags_main_and_merges_into_both() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);
        start_branch(&repo, &config, WorkflowBranchKind::Release, "1.0.0").unwrap();
        commit_file(&repo, "CHANGELOG.md", "1.0.0\n");

        let outcome = finish_branch(&repo, &config, WorkflowBranchKind::Release, "1.0.0").unwrap();

        assert!(matches!(outcome, FinishOutcome::Finished));
        assert!(branch::list_tags(&repo)
            .unwrap()
            .contains(&"1.0.0".to_string()));
        assert!(repo
            .find_branch("release/1.0.0", BranchType::Local)
            .is_err());

        for name in ["main", "develop"] {
            let branch = repo.find_branch(name, BranchType::Local).unwrap();
            let tree = repo
                .find_commit(branch.get().target().unwrap())
                .unwrap()
                .tree()
                .unwrap();
            assert!(
                tree.get_path(Path::new("CHANGELOG.md")).is_ok(),
                "{name} should contain the release's changes"
            );
        }
    }

    #[test]
    fn finish_branch_hotfix_creates_merge_commit_on_main_even_when_ff_possible() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);
        start_branch(&repo, &config, WorkflowBranchKind::Hotfix, "1.0.1").unwrap();
        commit_file(&repo, "hotfix.txt", "hotfix\n");

        let outcome = finish_branch(&repo, &config, WorkflowBranchKind::Hotfix, "1.0.1").unwrap();

        assert!(matches!(outcome, FinishOutcome::Finished));
        let main = repo.find_branch("main", BranchType::Local).unwrap();
        let main_tip = repo.find_commit(main.get().target().unwrap()).unwrap();
        assert_eq!(
            main_tip.parent_count(),
            2,
            "main should have a real merge commit, not a fast-forward"
        );
    }

    #[test]
    fn finish_branch_release_creates_merge_commits_on_main_and_develop_even_when_ff_possible() {
        let (_dir, repo) = repo_init();
        let config = init_default(&repo);
        start_branch(&repo, &config, WorkflowBranchKind::Release, "1.0.0").unwrap();
        commit_file(&repo, "CHANGELOG.md", "1.0.0\n");

        let outcome = finish_branch(&repo, &config, WorkflowBranchKind::Release, "1.0.0").unwrap();

        assert!(matches!(outcome, FinishOutcome::Finished));
        for name in ["main", "develop"] {
            let branch = repo.find_branch(name, BranchType::Local).unwrap();
            let tip = repo.find_commit(branch.get().target().unwrap()).unwrap();
            assert_eq!(
                tip.parent_count(),
                2,
                "{name} should have a real merge commit, not a fast-forward"
            );
        }
    }

    /// A conflicting finish pauses instead of proceeding, and re-invoking `finish_branch`
    /// after the caller resolves the conflict through the existing merge-conflict flow
    /// (`write_resolved_conflict` + a commit that finishes the merge, exactly like any other
    /// conflicted merge in this app) picks the remaining steps back up.
    #[test]
    fn finish_branch_pauses_on_conflict_and_resumes_when_re_invoked() {
        let (_dir, repo) = repo_init();
        commit_file(&repo, "shared.txt", "base\n");
        let config = init_default(&repo);

        start_branch(&repo, &config, WorkflowBranchKind::Feature, "conflict").unwrap();
        commit_file(&repo, "shared.txt", "feature change\n");

        branch::checkout_branch(&repo, "develop").unwrap();
        commit_file(&repo, "shared.txt", "develop change\n");

        let outcome =
            finish_branch(&repo, &config, WorkflowBranchKind::Feature, "conflict").unwrap();
        let FinishOutcome::Conflicts(conflicts) = outcome else {
            panic!("expected a conflict, got {outcome:?}");
        };
        assert_eq!(conflicts, vec!["shared.txt".to_string()]);
        assert!(repo
            .find_branch("feature/conflict", BranchType::Local)
            .is_ok());

        branch::write_resolved_conflict(&repo, "shared.txt", "resolved\n").unwrap();
        stage::commit(&repo, "Merge feature/conflict", false, true).unwrap();

        let outcome =
            finish_branch(&repo, &config, WorkflowBranchKind::Feature, "conflict").unwrap();

        assert!(matches!(outcome, FinishOutcome::Finished));
        assert!(repo
            .find_branch("feature/conflict", BranchType::Local)
            .is_err());
    }
}
