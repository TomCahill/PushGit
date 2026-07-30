// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! On-disk branch-color cache: keeps `ColorAssigner`'s color choices stable across app
//! restarts, not just within one session.
//!
//! Every failure mode here (can't resolve a data dir, missing/corrupt file, read-only
//! filesystem) degrades to "no cache" rather than a hard error — colors are always
//! re-derivable without it ("regenerates deterministically"), so a
//! cache problem should never be able to break the graph.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::repo_id;

/// Loads the cached `{branch_name: color_id}` map for `workdir`, or an empty map if there
/// is none yet, it can't be read, or it doesn't parse.
pub fn load(workdir: &Path) -> HashMap<String, u16> {
    match data_dir() {
        Some(dir) => load_from(&dir, workdir),
        None => HashMap::new(),
    }
}

/// Writes `colors` back to `workdir`'s cache file, creating parent directories as needed.
/// Best-effort — a write failure is silently ignored, matching this module's header comment.
pub fn save(workdir: &Path, colors: &HashMap<String, u16>) {
    if let Some(dir) = data_dir() {
        save_to(&dir, workdir, colors);
    }
}

fn data_dir() -> Option<PathBuf> {
    Some(
        directories::ProjectDirs::from("", "", "pushgit")?
            .data_dir()
            .to_path_buf(),
    )
}

fn cache_path(data_dir: &Path, workdir: &Path) -> PathBuf {
    data_dir.join(repo_id(workdir)).join("branch_colors.json")
}

fn load_from(data_dir: &Path, workdir: &Path) -> HashMap<String, u16> {
    let Ok(contents) = std::fs::read_to_string(cache_path(data_dir, workdir)) else {
        return HashMap::new();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

fn save_to(data_dir: &Path, workdir: &Path, colors: &HashMap<String, u16>) {
    let path = cache_path(data_dir, workdir);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(colors) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trips_colors_through_a_save_and_load() {
        let data_dir = TempDir::new().unwrap();
        let workdir = Path::new("/home/tom/some-repo");
        let mut colors = HashMap::new();
        colors.insert("main".to_string(), 0u16);
        colors.insert("feature/x".to_string(), 5u16);

        save_to(data_dir.path(), workdir, &colors);
        let loaded = load_from(data_dir.path(), workdir);

        assert_eq!(loaded, colors);
    }

    #[test]
    fn loading_a_never_saved_repo_returns_an_empty_map() {
        let data_dir = TempDir::new().unwrap();

        let loaded = load_from(data_dir.path(), Path::new("/never/saved"));

        assert!(loaded.is_empty());
    }

    #[test]
    fn different_workdirs_get_different_cache_files() {
        let data_dir = TempDir::new().unwrap();
        let mut a_colors = HashMap::new();
        a_colors.insert("main".to_string(), 0u16);

        save_to(data_dir.path(), Path::new("/repo/a"), &a_colors);
        let loaded_b = load_from(data_dir.path(), Path::new("/repo/b"));

        assert!(loaded_b.is_empty());
    }

    #[test]
    fn a_later_save_overwrites_the_earlier_one_for_the_same_repo() {
        let data_dir = TempDir::new().unwrap();
        let workdir = Path::new("/repo/a");
        let mut first = HashMap::new();
        first.insert("main".to_string(), 0u16);
        let mut second = HashMap::new();
        second.insert("main".to_string(), 1u16);
        second.insert("develop".to_string(), 2u16);

        save_to(data_dir.path(), workdir, &first);
        save_to(data_dir.path(), workdir, &second);
        let loaded = load_from(data_dir.path(), workdir);

        assert_eq!(loaded, second);
    }
}
