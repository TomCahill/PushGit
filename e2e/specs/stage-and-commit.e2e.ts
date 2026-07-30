// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Golden path 1: open a repo → stage a file → commit → see the new
// commit appear in the graph. Exercises the real window, real webview, and real IPC round
// trip that unit/component tests structurally can't reach.
import { initRepo, removeRepo, writeRepoFile } from "../helpers/gitFixture";

describe("stage and commit golden path", () => {
  let repoDir: string;

  beforeEach(async () => {
    repoDir = initRepo();
    // PushGit remembers the last-opened repo path in localStorage and reopens it on mount;
    // clearing it keeps each run's initial auto-load attempt from pointing at a previous
    // test's already-deleted fixture directory.
    await browser.execute(() => localStorage.clear());
  });

  afterEach(() => {
    removeRepo(repoDir);
  });

  it("opens a repo, stages a new file, commits it, and sees it appear in the graph", async () => {
    writeRepoFile(repoDir, "hello.txt", "hello from e2e\n");

    // The real folder picker is a native OS dialog WebDriver can't drive; inject the fixture
    // repo path via the E2E-only override `pickRepositoryFolder` checks for
    // (`src/lib/git/api.ts`) instead of showing a real one.
    await browser.execute((path) => {
      (window as unknown as { __e2eDialogPath__?: string }).__e2eDialogPath__ = path;
    }, repoDir);

    const openButton = await browser.$('button[title="Open a repository"]');
    await openButton.waitForExist({ timeout: 20_000 });
    await openButton.click();

    // The working-directory view lists the new file as unstaged.
    const fileEntry = await browser.$("span=hello.txt");
    await fileEntry.waitForExist({ timeout: 15_000 });

    await (await browser.$('button[title="Stage"]')).click();

    const title = await browser.$("#commit-title");
    await title.waitForExist();
    await title.setValue("Add hello.txt via E2E");

    const commitButton = await browser.$("button=Commit");
    await browser.waitUntil(async () => !(await commitButton.getAttribute("disabled")), {
      timeout: 8_000,
      timeoutMsg: "commit button never became enabled",
    });
    await commitButton.click();

    const commitRow = await browser.$("div*=Add hello.txt via E2E");
    await commitRow.waitForExist({ timeout: 15_000 });
    await expect(commitRow).toBeDisplayed();
  });
});
