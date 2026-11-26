// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};

use crate::diff::{BinaryPreview, FileStatus};

#[derive(Debug, Clone, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum TreeSource {
    Revision { oid: String },
    WorkingTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Submodule,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub status: Option<FileStatus>,
    pub contains_changes: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathEntry {
    pub path: String,
    pub status: Option<FileStatus>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreePaths {
    pub paths: Vec<PathEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum BlobView {
    Text {
        content: String,
        truncated: bool,
        byte_len: u64,
    },
    Image {
        preview: BinaryPreview,
    },
    Binary {
        byte_len: u64,
    },
    Symlink {
        target: String,
    },
    Submodule {
        commit: String,
    },
    Deleted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RefStatus {
    Unchanged,
    Moved {
        oid: String,
        newer_commits: Option<usize>,
    },
    Gone,
}
