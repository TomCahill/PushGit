# Contributing to PushGit

PushGit is a small, solo-maintained project. Contributions are welcome, but please read this
before opening a PR so we don't waste each other's time.

## Before you start

For anything beyond a small fix (a new feature, a UI change, a new dependency), please open an
issue first to discuss the approach. Design and scope decisions are made deliberately here — see
"Locked decisions" below and check the issue tracker for prior discussion before assuming a gap is
unintentional.

**Locked decisions** (please don't send PRs proposing alternatives without opening an issue first):

- Tauri (Rust core + OS webview), not Electron.
- Linux-first for v1. The architecture doesn't preclude macOS/Windows, but they're not a current goal.
- No cloud accounts, no telemetry, no phone-home. Ever.
- AGPL-3.0-or-later licensing for the whole project.

## Getting set up

**Prerequisites:** [Rust](https://www.rust-lang.org/tools/install), Node.js 20+ (repo pins v24, see
`.nvmrc`), and the [Tauri Linux system dependencies](https://v2.tauri.app/start/prerequisites/#linux).

```bash
git clone https://github.com/TomCahill/pushgit.git pushgit
cd pushgit
npm install
npm run tauri dev
```

## Common commands

| Command | What it does |
|---|---|
| `npm run dev` | Vite dev server (frontend only, no Rust backend) |
| `npm run check` | svelte-kit sync + svelte-check (TypeScript/Svelte types) |
| `npm test` / `npm run test:watch` | Vitest (frontend) |
| `npm run test:coverage` | Vitest with coverage |
| `npm run rust:check` / `rust:test` / `rust:clippy` / `rust:fmt` | Rust backend, run from repo root (they `cd src-tauri` internally) |
| `npm run test:all` | Frontend + Rust tests |
| `npm run test:e2e` | Builds a Tauri e2e binary and runs the WebdriverIO specs in `e2e/` |
| `npm run ci` | The full gate: Rust fmt check, clippy, Rust tests, Svelte check, frontend tests |

Run `npm run ci` before opening a PR. It's the same gate CI runs, so a clean local run means CI
should pass too.

## Code style

- Rust: `rustfmt` and `clippy` are enforced (`-D warnings`, no exceptions). Run `npm run rust:fmt`
  and `npm run rust:clippy` before committing.
- Frontend: Prettier is enforced (`npm run format:check` / `npm run format` to fix).
- Comments should explain *why*, not *what* — assume the reader knows the language. Skip comments
  that just restate the code.
- Don't add abstractions, config flags, or error handling for cases that can't happen. Match the
  scope of the change to the problem it's fixing.

## Tests

- New behavior needs test coverage: Rust unit/integration tests under `src-tauri/`, frontend tests
  with Vitest, and (for anything touching the main user-facing flows) a WebdriverIO spec under
  `e2e/` if one doesn't already cover it.
- Don't reduce existing coverage to make a change easier.

## Commit messages and PRs

- Keep commit subjects short (≤72 chars) and in the imperative mood (`fix: ...`, `feat: ...`,
  `refactor: ...`); put any additional context in the body, not the subject.
- Keep PRs focused on one change. Unrelated cleanup should be its own PR.
- Describe *why* the change is needed in the PR description, not just what changed — the diff
  already shows what changed.
- By submitting a contribution, you agree it's licensed under this project's
  [AGPL-3.0-or-later license](./LICENSE).

## Reporting bugs and requesting features

Use the GitHub issue tracker. For security vulnerabilities, see [SECURITY.md](./SECURITY.md)
instead of opening a public issue.
