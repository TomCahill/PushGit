<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="120" height="120" alt="PushGit logo">
</p>

<h1 align="center">PushGit</h1>
<p align="center">A fast, native git GUI with a proper visual commit graph.</p>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/TomCahill/pushgit.git/ci.yml?branch=main&label=CI" alt="CI status">
  <img src="https://img.shields.io/github/v/release/TomCahill/pushgit.git" alt="Latest release">
  <img src="https://img.shields.io/badge/platform-Linux-informational" alt="Platform: Linux">
  <img src="https://img.shields.io/badge/license-AGPL--3.0--or--later-blue" alt="License: AGPL-3.0-or-later">
</p>

Most git GUIs get the commit graph right, but also come bundled with Electron, a mandatory account, AI
features wired into every input box, and multi-gigabyte idle memory after a day of use. PushGit keeps
the part that's worth keeping a fast, clear visual history and the everyday git operations and
throws out everything else, by design, from the first commit.

## Features

- **Commit graph** - lanes, stable branch/tag colors, a live "uncommitted changes" row, search/filter,
  and solo/hide, built to stay smooth on large histories.
- **Staging & commits** - whole-file and hunk-level (and sub-hunk) stage/unstage, amend, commit message
  templates, and a skip-hooks toggle.
- **Diffing** - inline and side-by-side views with syntax highlighting, remembered per session.
- **Branching** - create, checkout, rename, delete (local and remote), merge, and non-interactive and
  interactive rebase, with a proper 3-way conflict resolution UI.
- **Stash** - create, apply, pop, drop, rename, and selectively stash individual hunks.
- **Remotes** - fetch, pull, and push over SSH and HTTPS, with ahead/behind tracking and upstream setup.
- **History tools** - blame, per-file history, and single- or multi-commit cherry-pick.
- **Safety net** - undo/redo across nearly every operation (commits, branch changes, merges, rebases,
  stash actions), backed by full repository snapshots rather than a single HEAD pointer.
- **Command palette** and light/dark theme following your OS setting.
- **A pure local git client** - no cloud accounts, no GitHub/GitLab/Jira integrations, no
  telemetry. It talks to whatever remote you've configured over plain SSH/HTTPS and nothing else.

## What it deliberately isn't

Not an all-in-one dev platform. No PR review, no issue tracking, no phone-home.

## Status

PushGit is under active development and Linux-first for now (the architecture doesn't preclude macOS/
Windows later). There are no packaged releases yet building from source is currently the only way to
run it.

## Getting started

**Prerequisites:** [Rust](https://www.rust-lang.org/tools/install), Node.js 20+, and the [Tauri Linux
system dependencies](https://v2.tauri.app/start/prerequisites/#linux).

```bash
git clone https://github.com/TomCahill/pushgit.git pushgit
cd pushgit
npm install
npm run tauri dev
```

To build a release binary instead:

```bash
npm run tauri build
```

## Tech stack

Tauri (Rust core + the OS's native webview) with a Svelte frontend, and
[`git2-rs`](https://github.com/rust-lang/git2-rs) (libgit2 bindings) instead of shelling out to the
`git` binary for every operation.

## License

Licensed under the [GNU Affero General Public License v3.0](./LICENSE) (AGPL-3.0-or-later). If you fork
PushGit or run a modified version as a network service, you must share your changes back.
