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

  it("renders an image preview for a binary image file when resolveImagePreview is given", async () => {
    const file = makeFileDiff({ isBinary: true, hunks: [], newPath: "logo.png" });
    const resolveImagePreview = vi.fn().mockResolvedValue({
      old: null,
      new: { kind: "content", base64: "AQID", byteLen: 3 },
    });

    const { findByAltText, queryByText } = render(HunkDiff, {
      props: { file, resolveImagePreview },
    });

    expect(await findByAltText("After")).toBeTruthy();
    expect(resolveImagePreview).toHaveBeenCalledWith(file);
    expect(queryByText("Binary file — no diff to show.")).toBeNull();
  });

  it("keeps the plain placeholder for a binary non-image file even with resolveImagePreview given", async () => {
    const file = makeFileDiff({ isBinary: true, hunks: [], newPath: "archive.zip" });
    const resolveImagePreview = vi.fn();

    const { findByText } = render(HunkDiff, { props: { file, resolveImagePreview } });

    expect(await findByText("Binary file — no diff to show.")).toBeTruthy();
    expect(resolveImagePreview).not.toHaveBeenCalled();
  });

  it("keeps the plain placeholder for a binary image file when resolveImagePreview is omitted", async () => {
    const file = makeFileDiff({ isBinary: true, hunks: [], newPath: "logo.png" });

    const { findByText } = render(HunkDiff, { props: { file } });

    expect(await findByText("Binary file — no diff to show.")).toBeTruthy();
  });

  it("falls back to the plain placeholder when the preview fetch rejects", async () => {
    const file = makeFileDiff({ isBinary: true, hunks: [], newPath: "logo.png" });
    const resolveImagePreview = vi.fn().mockRejectedValue(new Error("boom"));

    const { findByText } = render(HunkDiff, { props: { file, resolveImagePreview } });

    expect(await findByText("Binary file — no diff to show.")).toBeTruthy();
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

  it("shows no checkbox on a context line, and a pure-addition run stays independently selectable", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "context", content: "unchanged\n", oldLineno: 1, newLineno: 1 },
        { origin: "addition", content: "new a\n", oldLineno: null, newLineno: 2 },
        { origin: "addition", content: "new b\n", oldLineno: null, newLineno: 3 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    const onLineAction = vi.fn(() => Promise.resolve());

    const { container, findByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await findByText("unchanged", { exact: false });
    const checkboxes = container.querySelectorAll(
      'input[type="checkbox"]',
    ) as NodeListOf<HTMLInputElement>;
    expect(checkboxes).toHaveLength(2);

    await fireEvent.click(checkboxes[0]);
    expect(onLineAction).toHaveBeenCalledWith(hunk, [1]);
  });

  it("collapses a deletion+addition edited line onto a single checkbox that acts on both", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "context", content: "unchanged\n", oldLineno: 1, newLineno: 1 },
        { origin: "deletion", content: "old\n", oldLineno: 2, newLineno: null },
        { origin: "addition", content: "new\n", oldLineno: null, newLineno: 2 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    const onLineAction = vi.fn(() => Promise.resolve());

    const { container, findByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await findByText("unchanged", { exact: false });
    const checkboxes = container.querySelectorAll(
      'input[type="checkbox"]',
    ) as NodeListOf<HTMLInputElement>;
    // One checkbox for the whole edit, not one per deletion/addition line.
    expect(checkboxes).toHaveLength(1);

    await fireEvent.click(checkboxes[0]);
    expect(onLineAction).toHaveBeenCalledWith(hunk, [1, 2]);
  });

  it("checking a line immediately acts on just that line, disabling it while the action is in flight", async () => {
    const hunk = makeHunk({
      lines: [
        { origin: "addition", content: "line a\n", oldLineno: null, newLineno: 1 },
        { origin: "addition", content: "line b\n", oldLineno: null, newLineno: 2 },
      ],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    let resolveAction: () => void = () => {};
    const onLineAction = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveAction = resolve;
        }),
    );

    const { container, findByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await findByText("line a", { exact: false });
    const checkboxes = container.querySelectorAll(
      'input[type="checkbox"]',
    ) as NodeListOf<HTMLInputElement>;

    await fireEvent.click(checkboxes[0]);
    // Acts immediately on just the checked line, no separate confirmation step.
    expect(onLineAction).toHaveBeenCalledTimes(1);
    expect(onLineAction).toHaveBeenCalledWith(hunk, [0]);
    expect(checkboxes[0].checked).toBe(true);
    expect(checkboxes[0].disabled).toBe(true);
    // The other line is untouched.
    expect(checkboxes[1].checked).toBe(false);
    expect(checkboxes[1].disabled).toBe(false);

    resolveAction();
    await Promise.resolve();
    await Promise.resolve();
    expect(checkboxes[0].checked).toBe(false);
    expect(checkboxes[0].disabled).toBe(false);
  });

  it("ignores a second click on a line while its action is still in flight", async () => {
    const hunk = makeHunk({
      lines: [{ origin: "addition", content: "line a\n", oldLineno: null, newLineno: 1 }],
    });
    const file = makeFileDiff({ hunks: [hunk] });
    const onLineAction = vi.fn(() => new Promise<void>(() => {}));

    const { container, findByText } = render(HunkDiff, {
      props: { file, lineActionLabel: "Stage", onLineAction },
    });

    await findByText("line a", { exact: false });
    const checkbox = container.querySelector('input[type="checkbox"]')!;

    await fireEvent.click(checkbox);
    await fireEvent.click(checkbox);
    expect(onLineAction).toHaveBeenCalledOnce();
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
