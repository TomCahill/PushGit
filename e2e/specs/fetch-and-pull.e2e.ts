// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Golden path 3: fetch/pull against a local bare-repo fixture →
// ahead/behind indicators update correctly. The "remote" is a real bare repo
// (the standard fixture pattern) with no network access, advanced by a second clone pushing into it —
// standing in for a teammate's push, exactly like the Rust backend's own
// `fetch_and_push_round_trip_through_a_bare_remote` test.
import {
  addRemote,
  cloneRepo,
  commitAll,
  initBareRepo,
  initRepo,
  push,
  pushUpstream,
  removeRepo,
  writeRepoFile,
} from "../helpers/gitFixture";

describe("fetch and pull golden path", () => {
  let bareDir: string;
  let localDir: string;
  let otherCloneDir: string;

  beforeEach(async () => {
    bareDir = initBareRepo();

    localDir = initRepo();
    addRemote(localDir, "origin", bareDir);
    pushUpstream(localDir, "origin", "main");

    // Someone else clones the same "origin" and pushes a commit `localDir` doesn't know
    // about yet — `localDir`'s recorded origin/main stays stale until it fetches.
    otherCloneDir = cloneRepo(bareDir);
    writeRepoFile(otherCloneDir, "other.txt", "from someone else\n");
    commitAll(otherCloneDir, "Someone else's commit");
    push(otherCloneDir);

    await browser.execute(() => localStorage.clear());
  });

  afterEach(() => {
    removeRepo(bareDir);
    removeRepo(localDir);
    removeRepo(otherCloneDir);
  });

  it("fetches the new remote commit, updates ahead/behind, and pulls it in", async () => {
    // The real folder picker is a native OS dialog WebDriver can't drive; inject the fixture
    // repo path via the E2E-only override `pickRepositoryFolder` checks for
    // (`src/lib/git/api.ts`) instead of showing a real one.
    await browser.execute((path) => {
      (window as unknown as { __e2eDialogPath__?: string }).__e2eDialogPath__ = path;
    }, localDir);

    const openButton = await browser.$('button[title="Open a repository"]');
    await openButton.waitForExist({ timeout: 20_000 });
    await openButton.click();

    // Branches/tags/stashes are toolbar popovers now (`ActionRail.svelte`); `BranchSidebar`
    // still loads in the background regardless of open/closed (`Popover.svelte` keeps it
    // mounted), but its ahead/behind badge is only actually *visible* once the popover is
    // open, so open it before checking anything's displayed.
    const branchTrigger = await browser.$(".action-rail").then((el) => el.$("span=main"));
    await branchTrigger.waitForExist({ timeout: 15_000 });

    // Nothing has diverged from `localDir`'s point of view yet — no ahead/behind badge.
    expect(await (await browser.$(".ahead-behind")).isExisting()).toBe(false);

    const fetchButton = await browser.$("button=Fetch");
    await fetchButton.waitForExist({ timeout: 15_000 });
    await fetchButton.click();

    const aheadBehind = await browser.$(".ahead-behind*=↓1");
    await aheadBehind.waitForExist({ timeout: 15_000 });
    await branchTrigger.click();
    await expect(aheadBehind).toBeDisplayed();

    const pullButton = await browser.$("button=Pull");
    await browser.waitUntil(async () => !(await pullButton.getAttribute("disabled")), {
      timeout: 8_000,
      timeoutMsg: "pull button never became enabled",
    });
    await pullButton.click();

    const newCommitRow = await browser.$("div*=Someone else's commit");
    await newCommitRow.waitForExist({ timeout: 15_000 });
    await expect(newCommitRow).toBeDisplayed();
    await aheadBehind.waitForExist({ timeout: 15_000, reverse: true });
  });
});
