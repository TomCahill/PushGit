// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, waitFor, within } from "@testing-library/svelte";
import { diffViewState } from "./diffViewMode.svelte";
import HunkDiff from "./HunkDiff.svelte";
import { makeFileDiff, makeHunk } from "$lib/git/testFixtures";

beforeEach(() => {
  diffViewState.mode = "inline";
});

afterEach(() => {
  diffViewState.mode = "inline";
});

describe("HunkDiff", () => {
  it("renders a hunk's header and lines", async () => {
    const file = makeFileDiff({ hunks: [makeHunk({ header: "@@ -1,1 +1,2 @@" })] });

    const { findByText } = render(HunkDiff, { props: { file } });

    expect(await findByText("@@ -1,1 +1,2 @@")).toBeTruthy();
    expect(await findByText("hello", { exact: false })).toBeTruthy();
  });

  it("shows a placeholder instead of hunks for a binary file", async () => {
    const file = makeFileDiff({ isBinary: true, hunks: [] });

    const { findByText, queryByText } = render(HunkDiff, { props: { file } });

    expect(await findByText("Binary file — no diff to show.")).toBeTruthy();
    expect(queryByText("hello", { exact: false })).toBeNull();
  });

  it("renders no hunk action button when onHunkAction is omitted", async () => {
    const file = makeFileDiff();

    const { findByText, queryByText } = render(HunkDiff, { props: { file } });

    await findByText("hello", { exact: false });
    expect(queryByText("Stage hunk")).toBeNull();
  });

  it("calls onHunkAction with the clicked hunk when the action button is used", async () => {
    const hunk = makeHunk();
    const file = makeFileDiff({ hunks: [hunk] });
    const onHunkAction = vi.fn();

    const { findByText } = render(HunkDiff, {
      props: { file, hunkActionLabel: "Stage hunk", onHunkAction },
    });

    await fireEvent.click(await findByText("Stage hunk"));

    expect(onHunkAction).toHaveBeenCalledWith(hunk);
  });

  it("switches between inline and side-by-side rendering when the toggle is clicked", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "deletion", content: "old\n", oldLineno: 1, newLineno: null },
        { origin: "addition", content: "new\n", oldLineno: null, newLineno: 1 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });

    const { container, findByText } = render(HunkDiff, { props: { file } });

    expect(container.querySelectorAll(".split-row")).toHaveLength(0);

    await fireEvent.click(await findByText("Side-by-side view"));

    expect(container.querySelectorAll(".split-row")).toHaveLength(1);
    expect(await findByText("Inline view")).toBeTruthy();

    await fireEvent.click(await findByText("Inline view"));

    expect(container.querySelectorAll(".split-row")).toHaveLength(0);
  });

  it("pads the shorter side of an unbalanced change block with an empty cell", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "deletion", content: "removed\n", oldLineno: 1, newLineno: null },
        { origin: "addition", content: "added 1\n", oldLineno: null, newLineno: 1 },
        { origin: "addition", content: "added 2\n", oldLineno: null, newLineno: 2 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });

    const { container, findByText } = render(HunkDiff, { props: { file } });
    await fireEvent.click(await findByText("Side-by-side view"));

    const rows = container.querySelectorAll(".split-row");
    expect(rows).toHaveLength(2);
    const secondRowCells = rows[1].querySelectorAll(".split-cell");
    expect(secondRowCells[0].classList.contains("empty")).toBe(true);
    expect(secondRowCells[1].textContent).toContain("added 2");
  });

  it("shares the view-mode toggle across every HunkDiff instance in the app", async () => {
    const fileA = makeFileDiff({ newPath: "a.txt" });
    const fileB = makeFileDiff({ newPath: "b.txt" });

    const first = render(HunkDiff, { props: { file: fileA } });
    const second = render(HunkDiff, { props: { file: fileB } });

    await fireEvent.click(await within(first.container).findByText("Side-by-side view"));

    expect(await within(second.container).findByText("Inline view")).toBeTruthy();
  });

  it("renders no line checkboxes when onLineAction is omitted", async () => {
    const file = makeFileDiff();

    const { container, findByText } = render(HunkDiff, { props: { file } });

    await findByText("hello", { exact: false });
    expect(container.querySelectorAll('input[type="checkbox"]')).toHaveLength(0);
  });

  it("only shows checkboxes on addition/deletion lines, never context, when onLineAction is given", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "context", content: "unchanged\n", oldLineno: 1, newLineno: 1 },
        { origin: "deletion", content: "old\n", oldLineno: 2, newLineno: null },
        { origin: "addition", content: "new\n", oldLineno: null, newLineno: 2 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });

    const { container, findByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction: vi.fn() },
    });

    await findByText("unchanged", { exact: false });
    expect(container.querySelectorAll('input[type="checkbox"]')).toHaveLength(2);
  });

  it("shows a per-hunk action button only once a line is checked, with the running count", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "addition", content: "line a\n", oldLineno: null, newLineno: 1 },
        { origin: "addition", content: "line b\n", oldLineno: null, newLineno: 2 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    const onLineAction = vi.fn();

    const { container, findByText, queryByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await findByText("line a", { exact: false });
    expect(queryByText("Stage 1 line")).toBeNull();

    const checkboxes = container.querySelectorAll('input[type="checkbox"]');
    await fireEvent.click(checkboxes[0]);
    expect(await findByText("Stage 1 line")).toBeTruthy();

    await fireEvent.click(checkboxes[1]);
    expect(await findByText("Stage 2 lines")).toBeTruthy();

    await fireEvent.click(await findByText("Stage 2 lines"));
    expect(onLineAction).toHaveBeenCalledWith(hunk, [0, 1]);

    // Clicking the action clears that hunk's selection.
    expect(queryByText("Stage 2 lines", { exact: false })).toBeNull();
  });

  it("resets line selection when the file diff is replaced", async () => {
    const hunk = makeHunk({
      lines: [{ origin: "addition", content: "line a\n", oldLineno: null, newLineno: 1 }],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    const onLineAction = vi.fn();

    const { container, findByText, rerender, queryByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await fireEvent.click(container.querySelector('input[type="checkbox"]')!);
    expect(await findByText("Stage 1 line")).toBeTruthy();

    const nextHunk = makeHunk({
      lines: [{ origin: "addition", content: "line b\n", oldLineno: null, newLineno: 1 }],
    });
    await rerender({
      file: makeFileDiff({ hunks: [nextHunk] }),
      lineActionLabel: "Stage",
      onLineAction,
    });

    await findByText("line b", { exact: false });
    expect(queryByText("Stage 1 line")).toBeNull();
  });

  it("highlights recognized-language content and falls back to plain text otherwise", async () => {
    const highlighted = makeFileDiff({
      newPath: "file.ts",
      hunks: [
        makeHunk({
          lines: [{ origin: "addition", content: "const x = 1;\n", oldLineno: null, newLineno: 1 }],
        }),
      ],
    });

    const { container: highlightedContainer } = render(HunkDiff, { props: { file: highlighted } });
    await waitFor(() =>
      expect(highlightedContainer.querySelector(".hljs-keyword")?.textContent).toBe("const"),
    );

    const plain = makeFileDiff({
      newPath: "file.unknownext",
      hunks: [
        makeHunk({
          lines: [{ origin: "addition", content: "const x = 1;\n", oldLineno: null, newLineno: 1 }],
        }),
      ],
    });
    const { findByText, container: plainContainer } = render(HunkDiff, { props: { file: plain } });
    await findByText("const x = 1", { exact: false });
    expect(plainContainer.querySelector(".hljs-keyword")).toBeNull();
  });
});
