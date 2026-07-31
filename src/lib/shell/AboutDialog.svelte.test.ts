// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import { mockIPC } from "@tauri-apps/api/mocks";
import AboutDialog from "./AboutDialog.svelte";

describe("AboutDialog", () => {
  it("opens the Ko-fi link in the system browser instead of navigating the app window", async () => {
    const calls: unknown[] = [];
    mockIPC((cmd, args) => {
      switch (cmd) {
        case "plugin:app|name":
          return "PushGit";
        case "plugin:app|version":
          return "1.0.0";
        case "plugin:opener|open_url":
          calls.push(args);
          return null;
        default:
          throw new Error(`unexpected command ${cmd}`);
      }
    });

    const { getByAltText } = render(AboutDialog, { props: { open: true, onClose: () => {} } });
    const link = getByAltText("Buy Me a Coffee at ko-fi.com").closest("a")!;
    // `dispatchEvent`'s return value is `false` when a handler called `preventDefault()` —
    // confirms the app window's own navigation was actually stopped, not just that `openUrl`
    // was called alongside an unprevented navigation.
    const notPrevented = await fireEvent.click(link);

    expect(calls).toEqual([{ url: "https://ko-fi.com/S5I7247HRN", openWith: undefined }]);
    expect(notPrevented).toBe(false);
  });
});
