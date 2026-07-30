// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Commit graph extraction: `Revwalk`-based topological traversal, lane/column layout and
//! crossing-avoidance routing, pagination and checkpointed incremental resumption for large
//! histories.

mod color;
mod color_cache;
mod commit_graph_file;
mod lane;
mod model;
mod session;

pub use model::{CommitGraphPage, GraphFilter};
pub use session::GraphSessions;
