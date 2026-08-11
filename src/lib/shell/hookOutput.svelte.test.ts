// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { beforeEach, describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import HookOutputModal from "./HookOutputModal.svelte";
import {
  finishHookOutput,
  hookOutputState,
  pushHookOutputLine,
  startHookOutput,
} from "./hookOutput.svelte";

describe("hookOutput", () => {
  beforeEach(() => {
    hookOutputState.session = null;
  });

  it("renders nothing when there is no session", () => {
    const { container } = render(HookOutputModal);
    expect(container.querySelector(".dialog")).toBeNull();
  });

  it("shows the label, running status, and appended lines", async () => {
    const { findByText } = render(HookOutputModal);

    startHookOutput("Commit");
    pushHookOutputLine({ hook: "pre-commit", stream: "stdout", text: "checking style" });

    expect(await findByText("Commit")).toBeTruthy();
    expect(await findByText("Running…")).toBeTruthy();
    expect(await findByText("checking style")).toBeTruthy();
  });

  it("marks stderr lines distinctly from stdout", async () => {
    const { findByText } = render(HookOutputModal);

    startHookOutput("Commit");
    pushHookOutputLine({ hook: "pre-commit", stream: "stdout", text: "an ok line" });
    pushHookOutputLine({ hook: "pre-commit", stream: "stderr", text: "a warning line" });

    const okLine = await findByText("an ok line");
    const warningLine = await findByText("a warning line");
    expect(okLine.classList.contains("stderr")).toBe(false);
    expect(warningLine.classList.contains("stderr")).toBe(true);
  });

  it("switches to 'Done' once finished, keeping the transcript visible", async () => {
    const { findByText } = render(HookOutputModal);

    startHookOutput("Push");
    pushHookOutputLine({ hook: "push", stream: "stderr", text: "remote: ok" });
    finishHookOutput();

    expect(await findByText("Done")).toBeTruthy();
    expect(await findByText("remote: ok")).toBeTruthy();
  });

  it("stays visible across a session until start() is called again", () => {
    startHookOutput("Commit");
    pushHookOutputLine({ hook: "pre-commit", stream: "stdout", text: "one" });
    finishHookOutput();

    expect(hookOutputState.session?.lines).toHaveLength(1);

    startHookOutput("Commit");
    expect(hookOutputState.session?.lines).toHaveLength(0);
    expect(hookOutputState.session?.running).toBe(true);
  });

  it("closes without clearing anything but the session itself", async () => {
    const { findByLabelText, container } = render(HookOutputModal);

    startHookOutput("Commit");
    pushHookOutputLine({ hook: "pre-commit", stream: "stdout", text: "one" });

    await fireEvent.click(await findByLabelText("Close"));

    expect(hookOutputState.session).toBeNull();
    expect(container.querySelector(".dialog")).toBeNull();
  });

  it("closes on Escape", async () => {
    render(HookOutputModal);
    startHookOutput("Commit");

    await fireEvent.keyDown(window, { key: "Escape" });

    expect(hookOutputState.session).toBeNull();
  });
});
