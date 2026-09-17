// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import ImageDiff from "./ImageDiff.svelte";
import type { BinaryPreview } from "$lib/git/types";

const content = (base64: string, byteLen: number): BinaryPreview => ({
  kind: "content",
  base64,
  byteLen,
});

describe("ImageDiff", () => {
  it("renders 'No file' for a side that doesn't exist", async () => {
    const { findAllByText } = render(ImageDiff, {
      props: { oldPreview: null, newPreview: content("AQID", 3), mimeType: "image/png" },
    });

    expect(await findAllByText("No file")).toHaveLength(1);
  });

  it("renders a too-large caption without attempting an image", async () => {
    const { findByText, container } = render(ImageDiff, {
      props: {
        oldPreview: { kind: "tooLarge", byteLen: 25 * 1024 * 1024 },
        newPreview: null,
        mimeType: "image/png",
      },
    });

    expect(await findByText("25.0 MB — too large to preview")).toBeTruthy();
    expect(container.querySelectorAll("img")).toHaveLength(0);
  });

  it("renders an img with the correct data URI and updates dimensions on load", async () => {
    const { findByAltText, findByText } = render(ImageDiff, {
      props: {
        oldPreview: content("AQID", 3),
        newPreview: null,
        mimeType: "image/png",
        oldLabel: "Before",
      },
    });

    const img = (await findByAltText("Before")) as HTMLImageElement;
    expect(img.src).toBe("data:image/png;base64,AQID");

    Object.defineProperty(img, "naturalWidth", { value: 800, configurable: true });
    Object.defineProperty(img, "naturalHeight", { value: 600, configurable: true });
    await fireEvent.load(img);

    expect(await findByText("800 × 600 · 3 B")).toBeTruthy();
  });
});
