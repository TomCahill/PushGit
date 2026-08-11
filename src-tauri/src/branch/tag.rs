// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tag create/list/delete/move/rename.

use git2::Repository;

use super::invalid;
use crate::error::PushGitResult;

pub fn list_tags(repo: &Repository) -> PushGitResult<Vec<String>> {
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = std::str::from_utf8(name)
            .ok()
            .and_then(|n| n.strip_prefix("refs/tags/"))
        {
            tags.push(short.to_string());
        }
        true
    })?;
    Ok(tags)
}

/// Creates a tag at `at` (or HEAD if not given). An annotated tag is created when
/// `message` is provided; otherwise a lightweight tag.
pub fn create_tag(
    repo: &Repository,
    name: &str,
    at: Option<&str>,
    message: Option<&str>,
) -> PushGitResult<()> {
    let target = match at {
        Some(rev) => repo.revparse_single(rev)?,
        None => repo.head()?.peel(git2::ObjectType::Commit)?,
    };

    match message {
        Some(message) => {
            let signature = repo.signature()?;
            repo.tag(name, &target, &signature, message, false)?;
        }
        None => {
            repo.tag_lightweight(name, &target, false)?;
        }
    }

    Ok(())
}

pub fn delete_tag(repo: &Repository, name: &str) -> PushGitResult<()> {
    repo.tag_delete(name)?;
    Ok(())
}

/// Force-moves an existing tag to `to` (any commit-ish) — the graph's drag-a-tag-onto-a-
/// commit-or-branch-badge gesture. Preserves whichever kind the tag already was: an
/// annotated tag is re-created at the new target with its original message (a tag object is
/// immutable and encodes its target, so "moving" one really means creating a new tag object
/// and force-updating the ref to point at it); a lightweight tag is just re-pointed.
pub fn move_tag(repo: &Repository, name: &str, to: &str) -> PushGitResult<()> {
    let reference = repo.find_reference(&format!("refs/tags/{name}"))?;
    let immediate_oid = reference
        .target()
        .ok_or_else(|| invalid(format!("tag '{name}' has no direct target")))?;
    let target = repo.revparse_single(to)?;

    match repo.find_tag(immediate_oid) {
        Ok(existing) => {
            let message = existing.message().unwrap_or_default().to_string();
            let signature = repo.signature()?;
            repo.tag(name, &target, &signature, &message, true)?;
        }
        Err(_) => {
            repo.tag_lightweight(name, &target, true)?;
        }
    }

    Ok(())
}

/// Renames a tag, preserving its target and (for an annotated tag) its message — git has no
/// native tag-rename, so this creates `new_name` at `old_name`'s current target and then
/// deletes `old_name`. Creates before deleting so a name collision on `new_name` leaves
/// `old_name` intact rather than losing the tag.
pub fn rename_tag(repo: &Repository, old_name: &str, new_name: &str) -> PushGitResult<()> {
    let reference = repo.find_reference(&format!("refs/tags/{old_name}"))?;
    let immediate_oid = reference
        .target()
        .ok_or_else(|| invalid(format!("tag '{old_name}' has no direct target")))?;

    match repo.find_tag(immediate_oid) {
        Ok(existing) => {
            let message = existing.message().unwrap_or_default().to_string();
            let signature = repo.signature()?;
            let target = existing.target()?;
            repo.tag(new_name, &target, &signature, &message, false)?;
        }
        Err(_) => {
            let target = repo.find_object(immediate_oid, None)?;
            repo.tag_lightweight(new_name, &target, false)?;
        }
    }

    repo.tag_delete(old_name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn create_list_and_delete_lightweight_and_annotated_tags() {
        let (_dir, repo) = repo_init();

        create_tag(&repo, "v1.0.0", None, None).unwrap();
        create_tag(&repo, "v1.1.0", None, Some("release notes")).unwrap();

        let mut tags = list_tags(&repo).unwrap();
        tags.sort();
        assert_eq!(tags, vec!["v1.0.0".to_string(), "v1.1.0".to_string()]);

        delete_tag(&repo, "v1.0.0").unwrap();
        assert_eq!(list_tags(&repo).unwrap(), vec!["v1.1.0".to_string()]);
    }

    #[test]
    fn move_tag_repoints_a_lightweight_tag() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(&repo, "v1.0.0", Some(&first.to_string()), None).unwrap();

        move_tag(&repo, "v1.0.0", &second.to_string()).unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        assert_eq!(reference.target(), Some(second));
        // Still lightweight — moving it shouldn't turn it into an annotated tag.
        assert!(repo.find_tag(reference.target().unwrap()).is_err());
    }

    #[test]
    fn move_tag_repoints_an_annotated_tag_and_keeps_its_message() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(
            &repo,
            "v1.0.0",
            Some(&first.to_string()),
            Some("release notes"),
        )
        .unwrap();

        move_tag(&repo, "v1.0.0", &second.to_string()).unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        let tag_object = repo.find_tag(reference.target().unwrap()).unwrap();
        assert_eq!(tag_object.target_id(), second);
        assert_eq!(tag_object.message(), Some("release notes"));
    }

    #[test]
    fn move_tag_accepts_a_branch_name_as_the_target() {
        let (_dir, repo) = repo_init();
        let first = repo.head().unwrap().peel_to_commit().unwrap().id();
        let second = commit_file(&repo, "a.txt", "a");
        create_tag(&repo, "v1.0.0", Some(&first.to_string()), None).unwrap();

        move_tag(&repo, "v1.0.0", "main").unwrap();

        let reference = repo.find_reference("refs/tags/v1.0.0").unwrap();
        assert_eq!(reference.target(), Some(second));
    }

    #[test]
    fn rename_tag_renames_a_lightweight_tag_and_keeps_its_target() {
        let (_dir, repo) = repo_init();
        let commit = repo.head().unwrap().peel_to_commit().unwrap().id();
        create_tag(&repo, "v1.0.0", None, None).unwrap();

        rename_tag(&repo, "v1.0.0", "v1.0.0-renamed").unwrap();

        assert!(repo.find_reference("refs/tags/v1.0.0").is_err());
        let reference = repo.find_reference("refs/tags/v1.0.0-renamed").unwrap();
        assert_eq!(reference.target(), Some(commit));
        assert!(repo.find_tag(reference.target().unwrap()).is_err());
    }

    #[test]
    fn rename_tag_renames_an_annotated_tag_and_keeps_its_message() {
        let (_dir, repo) = repo_init();
        let commit = repo.head().unwrap().peel_to_commit().unwrap().id();
        create_tag(&repo, "v1.0.0", None, Some("release notes")).unwrap();

        rename_tag(&repo, "v1.0.0", "v1.0.0-renamed").unwrap();

        assert!(repo.find_reference("refs/tags/v1.0.0").is_err());
        let reference = repo.find_reference("refs/tags/v1.0.0-renamed").unwrap();
        let tag_object = repo.find_tag(reference.target().unwrap()).unwrap();
        assert_eq!(tag_object.target_id(), commit);
        assert_eq!(tag_object.message(), Some("release notes"));
    }

    #[test]
    fn rename_tag_leaves_the_old_tag_intact_when_the_new_name_is_taken() {
        let (_dir, repo) = repo_init();
        create_tag(&repo, "v1.0.0", None, None).unwrap();
        create_tag(&repo, "v2.0.0", None, None).unwrap();

        assert!(rename_tag(&repo, "v1.0.0", "v2.0.0").is_err());

        assert!(repo.find_reference("refs/tags/v1.0.0").is_ok());
        assert!(repo.find_reference("refs/tags/v2.0.0").is_ok());
    }
}
