// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { imageMimeType, isImagePath } from "./isImagePath";

describe("isImagePath", () => {
  it.each([
    ["logo.png", true],
    ["photo.jpg", true],
    ["photo.jpeg", true],
    ["anim.gif", true],
    ["banner.webp", true],
    ["cursor.bmp", true],
    ["favicon.ico", true],
    ["LOGO.PNG", true],
    ["archive.zip", false],
    ["doc.pdf", false],
    ["README", false],
    [null, false],
  ])("%s -> %s", (path, expected) => {
    expect(isImagePath(path)).toBe(expected);
  });
});

describe("imageMimeType", () => {
  it("maps known extensions to their MIME type", () => {
    expect(imageMimeType("logo.png")).toBe("image/png");
    expect(imageMimeType("photo.JPG")).toBe("image/jpeg");
    expect(imageMimeType("favicon.ico")).toBe("image/x-icon");
  });

  it("falls back to a generic type for an unrecognized extension", () => {
    expect(imageMimeType("archive.zip")).toBe("application/octet-stream");
    expect(imageMimeType("noext")).toBe("application/octet-stream");
  });
});
