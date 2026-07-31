// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { describe, expect, it } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import Avatar from "./Avatar.svelte";

describe("Avatar", () => {
  it("shows both initials for a two-word name", () => {
    const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });
    expect(container.querySelector(".avatar")?.textContent?.trim()).toBe("AL");
  });

  it("shows a single initial for a one-word name", () => {
    const { container } = render(Avatar, { props: { name: "Cher" } });
    expect(container.querySelector(".avatar")?.textContent?.trim()).toBe("C");
  });

  it("uses only the first two words for a many-word name", () => {
    const { container } = render(Avatar, { props: { name: "Ada Marie Lovelace" } });
    expect(container.querySelector(".avatar")?.textContent?.trim()).toBe("AM");
  });

  it("is hidden from assistive tech since the adjacent name text is the accessible label", () => {
    const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });
    expect(container.querySelector(".avatar")?.getAttribute("aria-hidden")).toBe("true");
  });

  it("renders the name as visible text alongside the initials circle", () => {
    const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });
    expect(container.querySelector(".name")?.textContent).toBe("Ada Lovelace");
  });

  it("renders the same color for the same email regardless of name", () => {
    const first = render(Avatar, {
      props: { name: "Ada Lovelace", email: "ada@example.com" },
    });
    const second = render(Avatar, {
      props: { name: "A. Lovelace", email: "ada@example.com" },
    });
    const firstBg = (first.container.querySelector(".avatar") as HTMLElement).style.background;
    const secondBg = (second.container.querySelector(".avatar") as HTMLElement).style.background;
    expect(firstBg).toBe(secondBg);
  });

  describe("hover card", () => {
    it("shows the name and email after a short hover delay", async () => {
      const { container } = render(Avatar, {
        props: { name: "Ada Lovelace", email: "ada@example.com" },
      });

      await fireEvent.mouseEnter(container.querySelector(".author-pill")!);
      expect(document.body.querySelector(".hover-card")).toBeNull();

      await waitFor(() => expect(document.body.querySelector(".hover-card")).toBeTruthy());
      const card = document.body.querySelector(".hover-card");
      expect(card?.querySelector(".hover-card-name")?.textContent).toBe("Ada Lovelace");
      expect(card?.querySelector(".hover-card-email")?.textContent).toBe("ada@example.com");
    });

    it("omits the email line when no email is given", async () => {
      const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });

      await fireEvent.mouseEnter(container.querySelector(".author-pill")!);

      await waitFor(() => expect(document.body.querySelector(".hover-card")).toBeTruthy());
      const card = document.body.querySelector(".hover-card");
      expect(card?.querySelector(".hover-card-name")?.textContent).toBe("Ada Lovelace");
      expect(card?.querySelector(".hover-card-email")).toBeNull();
    });

    it("does not show the card if the pointer leaves before the delay elapses", async () => {
      const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });

      const avatar = container.querySelector(".author-pill")!;
      await fireEvent.mouseEnter(avatar);
      await fireEvent.mouseLeave(avatar);

      await new Promise((resolve) => setTimeout(resolve, 350));
      expect(document.body.querySelector(".hover-card")).toBeNull();
    });

    it("hides the card immediately when the pointer leaves", async () => {
      const { container } = render(Avatar, { props: { name: "Ada Lovelace" } });

      const avatar = container.querySelector(".author-pill")!;
      await fireEvent.mouseEnter(avatar);
      await waitFor(() => expect(document.body.querySelector(".hover-card")).toBeTruthy());

      await fireEvent.mouseLeave(avatar);
      expect(document.body.querySelector(".hover-card")).toBeNull();
    });

    it("removes the portaled card from document.body when the component unmounts", async () => {
      const { container, unmount } = render(Avatar, { props: { name: "Ada Lovelace" } });

      await fireEvent.mouseEnter(container.querySelector(".author-pill")!);
      await waitFor(() => expect(document.body.querySelector(".hover-card")).toBeTruthy());

      unmount();
      expect(document.body.querySelector(".hover-card")).toBeNull();
    });
  });
});
