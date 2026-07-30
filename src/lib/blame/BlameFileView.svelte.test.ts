// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { render, waitFor } from "@testing-library/svelte";
import BlameFileView from "./BlameFileView.svelte";
import type { BlameLine } from "$lib/git/types";

function makeLine(overrides: Partial<BlameLine> = {}): BlameLine {
  return {
    lineNo: 1,
    content: "const x = 1;",
    oid: "aaa",
    shortOid: "aaa",
    authorName: "Ada",
    authorTime: 1700000000,
    summary: "first",
    ...overrides,
  };
}

describe("BlameFileView", () => {
  it("highlights recognized-language content and falls back to plain text otherwise", async () => {
    const lines = [makeLine()];

    const { container: highlightedContainer } = render(BlameFileView, {
      props: { lines, path: "file.ts" },
    });
    await waitFor(() =>
      expect(highlightedContainer.querySelector(".hljs-keyword")?.textContent).toBe("const"),
    );

    const { findByText, container: plainContainer } = render(BlameFileView, {
      props: { lines, path: "file.unknownext" },
    });
    await findByText("const x = 1;", { exact: false });
    expect(plainContainer.querySelector(".hljs-keyword")).toBeNull();
  });

  it("renders line number, author, and short oid alongside each line", async () => {
    const lines = [
      makeLine({ lineNo: 42, content: "hello", authorName: "Ada", shortOid: "abc1234" }),
    ];

    const { findByText } = render(BlameFileView, { props: { lines, path: "file.txt" } });

    expect(await findByText("42")).toBeTruthy();
    expect(await findByText("Ada")).toBeTruthy();
    expect(await findByText("abc1234")).toBeTruthy();
    expect(await findByText("hello")).toBeTruthy();
  });
});
