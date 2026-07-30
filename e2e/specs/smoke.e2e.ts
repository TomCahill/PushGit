// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Confirms the harness itself works end-to-end (real binary launches, WebDriver session
// connects, the real DOM is reachable) before the golden-path specs build on top of it.
describe("app shell", () => {
  it("launches and renders the open-repository button", async () => {
    const openButton = await browser.$('button[title="Open a repository"]');
    await openButton.waitForExist({ timeout: 20_000 });
    await expect(openButton).toBeDisplayed();
  });
});
