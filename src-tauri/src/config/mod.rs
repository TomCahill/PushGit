// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! App-level and per-repo settings storage. App
//! settings live in the XDG config dir; per-repo settings live in the XDG data dir next to
//! `graph::color_cache`'s branch-color cache, keyed by the same `repo_id()`.
//! Every failure mode (can't resolve a dir, missing/corrupt file, read-only filesystem)
//! degrades to defaults on read and is silently ignored on write, matching
//! `graph/color_cache.rs`'s established philosophy — a settings-storage problem should never
//! be able to block the app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ai::AiSettings;

pub const DEFAULT_MAX_COMMITS_RENDERED: u32 = 500;
pub const MIN_MAX_COMMITS_RENDERED: u32 = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub max_commits_rendered: u32,
    #[serde(default)]
    pub reduce_motion: bool,
    #[serde(default)]
    pub ai: AiSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            max_commits_rendered: DEFAULT_MAX_COMMITS_RENDERED,
            reduce_motion: false,
            ai: AiSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoConfig {
    pub default_skip_hooks: bool,
}

/// Clamps to `MIN_MAX_COMMITS_RENDERED`, matching this setting's pre-existing behavior from
/// when it lived in the frontend's `localStorage`.
pub fn clamp_max_commits_rendered(value: u32) -> u32 {
    value.max(MIN_MAX_COMMITS_RENDERED)
}

pub fn load_app_config() -> AppConfig {
    match config_dir() {
        Some(dir) => load_app_config_from(&dir),
        None => AppConfig::default(),
    }
}

/// Best-effort — a write failure is silently ignored, matching this module's header comment.
pub fn save_app_config(config: &AppConfig) {
    if let Some(dir) = config_dir() {
        save_app_config_to(&dir, config);
    }
}

pub fn load_repo_config(workdir: &Path) -> RepoConfig {
    match data_dir() {
        Some(dir) => load_repo_config_from(&dir, workdir),
        None => RepoConfig::default(),
    }
}

/// Best-effort — a write failure is silently ignored, matching this module's header comment.
pub fn save_repo_config(workdir: &Path, config: &RepoConfig) {
    if let Some(dir) = data_dir() {
        save_repo_config_to(&dir, workdir, config);
    }
}

/// `<repo-id>` for the `<data-dir>/<repo-id>/...` per-repo files:
/// `workdir`'s absolute path with every non-alphanumeric byte replaced by `_`. Shared with
/// `graph::color_cache`, which stores the branch-color cache in this same per-repo directory.
pub fn repo_id(workdir: &Path) -> String {
    workdir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn config_dir() -> Option<PathBuf> {
    Some(
        directories::ProjectDirs::from("", "", "pushgit")?
            .config_dir()
            .to_path_buf(),
    )
}

fn data_dir() -> Option<PathBuf> {
    Some(
        directories::ProjectDirs::from("", "", "pushgit")?
            .data_dir()
            .to_path_buf(),
    )
}

/// The downloaded local-AI model + engine live here, not under `cache_dir()` — unlike the
/// derived/regenerable data the cache dir is documented for, these are large, slow-to-refetch
/// downloads the local-AI feature actively depends on working.
pub fn local_ai_dir() -> Option<PathBuf> {
    Some(data_dir()?.join("local-ai"))
}

fn app_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("config.json")
}

fn repo_config_path(data_dir: &Path, workdir: &Path) -> PathBuf {
    data_dir.join(repo_id(workdir)).join("repo_settings.json")
}

fn load_app_config_from(config_dir: &Path) -> AppConfig {
    read_json(&app_config_path(config_dir)).unwrap_or_default()
}

fn save_app_config_to(config_dir: &Path, config: &AppConfig) {
    write_json(&app_config_path(config_dir), config);
}

fn load_repo_config_from(data_dir: &Path, workdir: &Path) -> RepoConfig {
    read_json(&repo_config_path(data_dir, workdir)).unwrap_or_default()
}

fn save_repo_config_to(data_dir: &Path, workdir: &Path, config: &RepoConfig) {
    write_json(&repo_config_path(data_dir, workdir), config);
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(value) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn app_config_defaults_when_no_file_exists() {
        let dir = TempDir::new().unwrap();

        let loaded = load_app_config_from(dir.path());

        assert_eq!(loaded, AppConfig::default());
    }

    #[test]
    fn app_config_round_trips_through_a_save_and_load() {
        let dir = TempDir::new().unwrap();
        let config = AppConfig {
            max_commits_rendered: 1234,
            reduce_motion: true,
            ai: AiSettings {
                transport: Some(crate::ai::AiTransport::OpenAiCompatible {
                    base_url: "http://localhost:11434/v1".to_string(),
                    model: "llama3.1".to_string(),
                }),
                instructions: "Use Conventional Commits.".to_string(),
                cloud_warning_acknowledged: true,
            },
        };

        save_app_config_to(dir.path(), &config);
        let loaded = load_app_config_from(dir.path());

        assert_eq!(loaded, config);
    }

    #[test]
    fn app_config_deserializes_reduce_motion_and_ai_missing_from_an_older_file_as_defaults() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            app_config_path(dir.path()),
            r#"{"maxCommitsRendered":1234}"#,
        )
        .unwrap();

        let loaded = load_app_config_from(dir.path());

        assert_eq!(
            loaded,
            AppConfig {
                max_commits_rendered: 1234,
                reduce_motion: false,
                ai: AiSettings::default(),
            }
        );
    }

    #[test]
    fn repo_config_defaults_when_no_file_exists() {
        let data_dir = TempDir::new().unwrap();

        let loaded = load_repo_config_from(data_dir.path(), Path::new("/never/saved"));

        assert_eq!(loaded, RepoConfig::default());
        assert!(!loaded.default_skip_hooks);
    }

    #[test]
    fn repo_config_round_trips_through_a_save_and_load() {
        let data_dir = TempDir::new().unwrap();
        let workdir = Path::new("/repo/a");
        let config = RepoConfig {
            default_skip_hooks: true,
        };

        save_repo_config_to(data_dir.path(), workdir, &config);
        let loaded = load_repo_config_from(data_dir.path(), workdir);

        assert_eq!(loaded, config);
    }

    #[test]
    fn different_workdirs_get_different_repo_config_files() {
        let data_dir = TempDir::new().unwrap();
        save_repo_config_to(
            data_dir.path(),
            Path::new("/repo/a"),
            &RepoConfig {
                default_skip_hooks: true,
            },
        );

        let loaded_b = load_repo_config_from(data_dir.path(), Path::new("/repo/b"));

        assert_eq!(loaded_b, RepoConfig::default());
    }

    #[test]
    fn repo_id_only_uses_filesystem_safe_characters() {
        let id = repo_id(Path::new("/home/tom/Projects/my repo (2)"));
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }

    #[test]
    fn clamp_max_commits_rendered_enforces_the_floor() {
        assert_eq!(clamp_max_commits_rendered(10), MIN_MAX_COMMITS_RENDERED);
        assert_eq!(clamp_max_commits_rendered(1000), 1000);
    }
}
