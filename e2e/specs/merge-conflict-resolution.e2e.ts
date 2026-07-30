// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Golden path 2: create a branch that diverges from main → merge it
// back → resolve the injected conflict through the real 3-way conflict editor
// (`src/lib/conflict/ConflictEditor.svelte`) → commit. Drives StagingPanel's "Resolve"
// button on the conflicted file, "Take theirs" for the single-hunk conflict, then "Mark
// resolved" — `write_resolved_conflict` stages it under the hood, so no separate stage
// click is needed before committing, unlike the old "edit externally + stage" workaround.
import {
  checkoutBranch,
  commitAll,
  createBranch,
  initRepo,
  removeRepo,
  writeRepoFile,
} from "../helpers/gitFixture";

describe("merge conflict resolution golden path", () => {
  let repoDir: string;

  beforeEach(async () => {
    repoDir = initRepo();
    writeRepoFile(repoDir, "shared.txt", "base\n");
    commitAll(repoDir, "Add shared.txt");
    createBranch(repoDir, "feature");

    writeRepoFile(repoDir, "shared.txt", "main version\n");
    commitAll(repoDir, "Update shared.txt on main");

    checkoutBranch(repoDir, "feature");
    writeRepoFile(repoDir, "shared.txt", "feature version\n");
    commitAll(repoDir, "Update shared.txt on feature");
    checkoutBranch(repoDir, "main");

    await browser.execute(() => localStorage.clear());
  });

  afterEach(() => {
    removeRepo(repoDir);
  });

  it("merges a diverging branch, resolves the conflict, and commits the result", async () => {
    // The real folder picker is a native OS dialog WebDriver can't drive; inject the fixture
    // repo path via the E2E-only override `pickRepositoryFolder` checks for
    // (`src/lib/git/api.ts`) instead of showing a real one.
    await browser.execute((path) => {
      (window as unknown as { __e2eDialogPath__?: string }).__e2eDialogPath__ = path;
    }, repoDir);

    const openButton = await browser.$('button[title="Open a repository"]');
    await openButton.waitForExist({ timeout: 20_000 });
    await openButton.click();

    const mergeButton = await browser.$('button[title="Merge feature into the current branch"]');
    await mergeButton.waitForExist({ timeout: 15_000 });
    await mergeButton.click();

    const conflictBanner = await browser.$("p*=Merge in progress");
    await conflictBanner.waitForExist({ timeout: 15_000 });

    const resolveButton = await browser.$('button[title="Resolve conflict"]');
    await resolveButton.waitForExist({ timeout: 15_000 });
    await resolveButton.click();

    const takeTheirsButton = await browser.$("button=Take theirs");
    await takeTheirsButton.waitForExist({ timeout: 15_000 });
    await takeTheirsButton.click();

    const markResolvedButton = await browser.$("button=Mark resolved");
    await browser.waitUntil(async () => !(await markResolvedButton.getAttribute("disabled")), {
      timeout: 8_000,
      timeoutMsg: "Mark resolved button never became enabled",
    });
    await markResolvedButton.click();

    const message = await browser.$("#commit-message");
    await message.waitForExist();
    await message.setValue("Resolve merge conflict via E2E");

    const commitButton = await browser.$("button=Commit");
    await browser.waitUntil(async () => !(await commitButton.getAttribute("disabled")), {
      timeout: 8_000,
      timeoutMsg: "commit button never became enabled",
    });
    await commitButton.click();

    await conflictBanner.waitForExist({ timeout: 15_000, reverse: true });

    const commitRow = await browser.$("div*=Resolve merge conflict via E2E");
    await commitRow.waitForExist({ timeout: 15_000 });
    await expect(commitRow).toBeDisplayed();
  });
});
