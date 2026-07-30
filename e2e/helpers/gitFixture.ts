// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Real git repo fixtures for E2E specs — shells out to the actual
// `git` binary rather than mocking, matching the project's "real git operations, not mocks"
// testing philosophy (see the Rust backend's `test_support::repo_init`).
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

function git(cwd: string, ...args: string[]): void {
  execFileSync("git", args, { cwd, stdio: "pipe" });
}

/** Creates a fresh repo with one commit (`README.md`) on `main`, returning its path. */
export function initRepo(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "pushgit-e2e-"));
  git(dir, "init", "--initial-branch=main");
  git(dir, "config", "user.email", "e2e@pushgit.test");
  git(dir, "config", "user.name", "PushGit E2E");
  writeFileSync(path.join(dir, "README.md"), "# Fixture repo\n");
  git(dir, "add", "README.md");
  git(dir, "commit", "-m", "Initial commit");
  return dir;
}

export function writeRepoFile(repoDir: string, relPath: string, content: string): void {
  writeFileSync(path.join(repoDir, relPath), content);
}

export function commitAll(repoDir: string, message: string): void {
  git(repoDir, "add", "-A");
  git(repoDir, "commit", "-m", message);
}

export function createBranch(repoDir: string, name: string): void {
  git(repoDir, "branch", name);
}

export function checkoutBranch(repoDir: string, name: string): void {
  git(repoDir, "checkout", name);
}

export function currentCommitSummaries(repoDir: string, count: number): string[] {
  const out = execFileSync("git", ["log", `-${count}`, "--format=%s"], {
    cwd: repoDir,
    encoding: "utf8",
  });
  return out.trim().split("\n").filter(Boolean);
}

export function removeRepo(repoDir: string): void {
  rmSync(repoDir, { recursive: true, force: true });
}

/** A bare repo with no working tree, standing in for a real "origin" (§3.2's fixture). */
export function initBareRepo(): string {
  const dir = mkdtempSync(path.join(tmpdir(), "pushgit-e2e-bare-"));
  git(dir, "init", "--bare", "--initial-branch=main");
  return dir;
}

export function addRemote(repoDir: string, name: string, url: string): void {
  git(repoDir, "remote", "add", name, url);
}

/** Pushes `branchName` to `remoteName`, configuring it as the branch's upstream. */
export function pushUpstream(repoDir: string, remoteName: string, branchName: string): void {
  git(repoDir, "push", "-u", remoteName, branchName);
}

export function push(repoDir: string): void {
  git(repoDir, "push");
}

/** Clones `sourceDir` into a fresh temp directory, configured for commits, and returns it. */
export function cloneRepo(sourceDir: string): string {
  const dir = mkdtempSync(path.join(tmpdir(), "pushgit-e2e-clone-"));
  git(dir, "clone", sourceDir, ".");
  git(dir, "config", "user.email", "e2e@pushgit.test");
  git(dir, "config", "user.name", "PushGit E2E");
  return dir;
}
