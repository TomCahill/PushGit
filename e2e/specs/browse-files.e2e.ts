// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Browse files shows a tag's content, not the working tree's, without checking anything out.
import { commitAll, initRepo, removeRepo, tag, writeRepoFile } from "../helpers/gitFixture";

describe("browse files", () => {
  let repoDir: string;

  beforeEach(async () => {
    repoDir = initRepo();
    await browser.execute(() => localStorage.clear());
  });

  afterEach(() => {
    removeRepo(repoDir);
  });

  async function fileText(): Promise<string> {
    const source = await browser.$("pre.source");
    await source.waitForExist({ timeout: 15_000 });
    return source.getText();
  }

  it("shows the working tree, then a tag's version of the same file", async () => {
    writeRepoFile(repoDir, "notes.txt", "version one\n");
    commitAll(repoDir, "Add notes");
    tag(repoDir, "v1.0");
    writeRepoFile(repoDir, "notes.txt", "version two\n");
    commitAll(repoDir, "Update notes");

    await browser.execute((path) => {
      (window as unknown as { __e2eDialogPath__?: string }).__e2eDialogPath__ = path;
    }, repoDir);
    const openButton = await browser.$('button[title="Open a repository"]');
    await openButton.waitForExist({ timeout: 20_000 });
    await openButton.click();
    await (await browser.$("div*=Update notes")).waitForExist({ timeout: 15_000 });

    await browser.keys(["Control", "p"]);
    const search = await browser.$('input[aria-label="Command palette search"]');
    await search.waitForExist();
    await search.setValue("Browse Files");
    await browser.keys("Enter");

    const row = await browser.$('div[role="treeitem"][data-path="notes.txt"]');
    await row.waitForExist({ timeout: 15_000 });
    await row.click();
    await browser.waitUntil(async () => (await fileText()).includes("version two"), {
      timeout: 10_000,
      timeoutMsg: "working tree content never showed",
    });

    await (await browser.$('button[aria-haspopup="listbox"]')).click();
    const refInput = await browser.$('input[aria-label="Browse files at"]');
    await refInput.waitForExist();
    await refInput.setValue("v1.0");
    await browser.keys("Enter");

    await browser.waitUntil(async () => (await fileText()).includes("version one"), {
      timeout: 10_000,
      timeoutMsg: "tag content never showed",
    });
  });
});
