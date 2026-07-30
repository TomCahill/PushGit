// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Placeholder proving the criterion harness works end-to-end. Will benchmark the
//! lane-assignment sweep against the synthetic 10k/100k-commit
//! fixtures once `graph/` exists.

use criterion::{criterion_group, criterion_main, Criterion};
use pushgit_lib::repo;
use tempfile::TempDir;

fn bench_repo_open(c: &mut Criterion) {
    let dir = TempDir::new().unwrap();
    git2::Repository::init(dir.path()).unwrap();

    c.bench_function("repo::open", |b| {
        b.iter(|| repo::open(dir.path()).unwrap());
    });
}

criterion_group!(benches, bench_repo_open);
criterion_main!(benches);
