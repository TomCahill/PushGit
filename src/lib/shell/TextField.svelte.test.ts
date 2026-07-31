// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/svelte";
import TextField from "./TextField.svelte";

describe("TextField", () => {
  it("associates a visible label with the input via id/for", () => {
    const { getByLabelText } = render(TextField, {
      props: { id: "max-commits", label: "Max commits rendered in graph", value: "500" },
    });
    const input = getByLabelText("Max commits rendered in graph") as HTMLInputElement;
    expect(input.value).toBe("500");
  });

  it("falls back to aria-label with no visible label chrome", () => {
    const { getByLabelText, queryByText } = render(TextField, {
      props: { ariaLabel: "Main branch name", value: "main" },
    });
    expect((getByLabelText("Main branch name") as HTMLInputElement).value).toBe("main");
    expect(queryByText("Main branch name")).toBeNull();
  });

  it("renders a password input when type=password", () => {
    const { getByLabelText } = render(TextField, {
      props: { id: "api-key", label: "API key", type: "password", value: "" },
    });
    expect((getByLabelText("API key") as HTMLInputElement).type).toBe("password");
  });

  it("renders a textarea when multiline", () => {
    const { getByLabelText } = render(TextField, {
      props: { id: "instructions", label: "Custom instructions", multiline: true, value: "hi" },
    });
    expect(getByLabelText("Custom instructions").tagName).toBe("TEXTAREA");
  });

  it("fires oninput and shows hint text", async () => {
    const oninput = vi.fn();
    const { getByLabelText, getByText } = render(TextField, {
      props: { id: "x", label: "X", value: "", hint: "Saved.", oninput },
    });
    await fireEvent.input(getByLabelText("X"), { target: { value: "a" } });
    expect(oninput).toHaveBeenCalledOnce();
    expect(getByText("Saved.")).toBeTruthy();
  });
});
