// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

import { afterEach, describe, expect, it } from "vitest";
import { fireEvent, render, waitFor } from "@testing-library/svelte";
import { within } from "@testing-library/dom";
import { mockIPC } from "@tauri-apps/api/mocks";
import SettingsPanel from "./SettingsPanel.svelte";
import {
  DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
  DEFAULT_MAX_COMMITS_RENDERED,
  MIN_MAX_COMMITS_RENDERED,
  settingsState,
} from "./settings.svelte";
import { toastState } from "$lib/shell/toast.svelte";

describe("SettingsPanel", () => {
  afterEach(() => {
    settingsState.maxCommitsRendered = DEFAULT_MAX_COMMITS_RENDERED;
    settingsState.reduceMotion = false;
    settingsState.ai = { transport: null, instructions: "", cloudWarningAcknowledged: false };
    settingsState.autoFetchEnabled = false;
    settingsState.autoFetchIntervalMinutes = DEFAULT_AUTO_FETCH_INTERVAL_MINUTES;
    settingsState.showHookOutputAlways = true;
    settingsState.hasAiApiKey = false;
    settingsState.localAiStatus = { modelPresent: false, enginePresent: false, gpuDevice: null };
    settingsState.localAiDownloadProgress = null;
    toastState.toasts = [];
  });

  it("shows the current max-commits-rendered value", () => {
    settingsState.maxCommitsRendered = 750;
    mockIPC(() => {
      throw new Error("should not be called when no repo is open");
    });

    const { getByLabelText } = render(SettingsPanel);

    expect((getByLabelText("Max commits rendered in graph") as HTMLInputElement).value).toBe("750");
  });

  it("does not show a 'This Repository' section when no repo is open", () => {
    mockIPC(() => {
      throw new Error("should not be called when no repo is open");
    });

    const { queryByText } = render(SettingsPanel);

    expect(queryByText("This Repository")).toBeNull();
  });

  it("saves a new max-commits value and shows a confirmation", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_max_commits_rendered") {
        expect(args).toEqual({ value: 1000 });
        return { maxCommitsRendered: 1000 };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const input = getByLabelText("Max commits rendered in graph") as HTMLInputElement;
    const form = within(input.closest("form")!);

    await fireEvent.input(input, { target: { value: "1000" } });
    await fireEvent.click(form.getByText("Save"));

    expect(await waitFor(() => form.getByText("Saved."))).toBeTruthy();
  });

  it("clamps a value below the minimum on save", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_max_commits_rendered") {
        return { maxCommitsRendered: MIN_MAX_COMMITS_RENDERED };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const input = getByLabelText("Max commits rendered in graph") as HTMLInputElement;
    const form = within(input.closest("form")!);

    await fireEvent.input(input, { target: { value: "5" } });
    await fireEvent.click(form.getByText("Save"));

    await waitFor(() => expect(settingsState.maxCommitsRendered).toBe(MIN_MAX_COMMITS_RENDERED));
    expect(input.value).toBe(String(MIN_MAX_COMMITS_RENDERED));
  });

  it("saves the reduce-motion preference as soon as the checkbox is toggled", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_reduce_motion") {
        expect(args).toEqual({ value: true });
        return { maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED, reduceMotion: true };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Reduce motion") as HTMLInputElement;

    await fireEvent.click(checkbox);

    expect(checkbox.checked).toBe(true);
  });

  it("reverts the reduce-motion checkbox and shows an error toast when the save fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_reduce_motion") throw "disk full";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Reduce motion") as HTMLInputElement;

    await fireEvent.click(checkbox);

    await waitFor(() => expect(checkbox.checked).toBe(false));
    expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
  });

  it("saves the show-hook-output-always preference as soon as the checkbox is toggled", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "set_show_hook_output_always") {
        expect(args).toEqual({ value: false });
        return { maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED, showHookOutputAlways: false };
      }
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Always show hook output") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);

    await fireEvent.click(checkbox);

    expect(checkbox.checked).toBe(false);
  });

  it("reverts the show-hook-output-always checkbox and shows an error toast when the save fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "set_show_hook_output_always") throw "disk full";
      throw new Error(`unexpected command ${cmd}`);
    });

    const { getByLabelText } = render(SettingsPanel);
    const checkbox = getByLabelText("Always show hook output") as HTMLInputElement;

    await fireEvent.click(checkbox);

    await waitFor(() => expect(checkbox.checked).toBe(true));
    expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
  });

  describe("auto-fetch", () => {
    it("hides the interval field until auto-fetch is turned on", () => {
      mockIPC(() => {
        throw new Error("should not be called when no repo is open");
      });

      const { queryByLabelText } = render(SettingsPanel);

      expect(queryByLabelText("Auto-fetch interval")).toBeNull();
    });

    it("saves the auto-fetch preference and reveals the interval field", async () => {
      mockIPC((cmd, args) => {
        if (cmd === "set_auto_fetch_enabled") {
          expect(args).toEqual({ value: true });
          return {
            maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
            reduceMotion: false,
            autoFetchEnabled: true,
            autoFetchIntervalMinutes: DEFAULT_AUTO_FETCH_INTERVAL_MINUTES,
          };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText, queryByLabelText } = render(SettingsPanel);
      const checkbox = getByLabelText("Auto-fetch") as HTMLInputElement;

      await fireEvent.click(checkbox);

      expect(checkbox.checked).toBe(true);
      expect(
        (await waitFor(() => queryByLabelText("Auto-fetch interval"))) as HTMLSelectElement,
      ).toBeTruthy();
    });

    it("reverts the auto-fetch checkbox and shows an error toast when the save fails", async () => {
      mockIPC((cmd) => {
        if (cmd === "set_auto_fetch_enabled") throw "disk full";
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const checkbox = getByLabelText("Auto-fetch") as HTMLInputElement;

      await fireEvent.click(checkbox);

      await waitFor(() => expect(checkbox.checked).toBe(false));
      expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
    });

    it("persists a new interval selection", async () => {
      settingsState.autoFetchEnabled = true;
      mockIPC((cmd, args) => {
        if (cmd === "set_auto_fetch_interval_minutes") {
          expect(args).toEqual({ value: 30 });
          return {
            maxCommitsRendered: DEFAULT_MAX_COMMITS_RENDERED,
            reduceMotion: false,
            autoFetchEnabled: true,
            autoFetchIntervalMinutes: 30,
          };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const select = getByLabelText("Auto-fetch interval") as HTMLSelectElement;

      await fireEvent.change(select, { target: { value: "30" } });

      await waitFor(() => expect(settingsState.autoFetchIntervalMinutes).toBe(30));
    });
  });

  describe("AI section", () => {
    it("is shown even when no repo is open", () => {
      mockIPC(() => {
        throw new Error("should not be called when no repo is open");
      });

      const { getByText } = render(SettingsPanel);

      expect(getByText("AI")).toBeTruthy();
    });

    it("saves the chosen provider, base URL, and model together", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "set_ai_transport") {
          calls.push(args);
          return {
            transport: { kind: "openAiCompatible", baseUrl: "http://localhost:11434/v1", model: "llama3.1" },
            instructions: "",
            cloudWarningAcknowledged: false,
          };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const providerSelect = getByLabelText("Provider") as HTMLSelectElement;
      const form = within(providerSelect.closest("form")!);

      await fireEvent.change(providerSelect, { target: { value: "openAiCompatible" } });
      await fireEvent.input(getByLabelText("Base URL"), {
        target: { value: "http://localhost:11434/v1" },
      });
      await fireEvent.input(getByLabelText("Model"), { target: { value: "llama3.1" } });
      await fireEvent.click(form.getByText("Save"));

      expect(await waitFor(() => form.getByText("Saved."))).toBeTruthy();
      expect(calls).toEqual([
        {
          transport: {
            kind: "openAiCompatible",
            baseUrl: "http://localhost:11434/v1",
            model: "llama3.1",
          },
        },
      ]);
    });

    it("prefills Anthropic's default base URL when switching to it", async () => {
      mockIPC(() => {
        throw new Error("should not be called before Save is clicked");
      });

      const { getByLabelText } = render(SettingsPanel);
      const providerSelect = getByLabelText("Provider") as HTMLSelectElement;

      await fireEvent.change(providerSelect, { target: { value: "anthropic" } });

      expect((getByLabelText("Base URL") as HTMLInputElement).value).toBe(
        "https://api.anthropic.com",
      );
    });

    it("does not show base URL/model fields when no provider is selected", () => {
      mockIPC(() => {
        throw new Error("should not be called when no repo is open");
      });

      const { queryByLabelText } = render(SettingsPanel);

      expect(queryByLabelText("Base URL")).toBeNull();
      expect(queryByLabelText("Model")).toBeNull();
    });

    it("saves a new API key and shows a saved indicator instead of the key's value", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "set_ai_api_key") {
          calls.push(args);
          return null;
        }
        if (cmd === "has_ai_api_key") return true;
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const input = getByLabelText("API key (optional for local servers)") as HTMLInputElement;
      const form = within(input.closest("form")!);

      await fireEvent.input(input, { target: { value: "sk-test" } });
      await fireEvent.click(form.getByText("Save"));

      expect(await waitFor(() => form.getByText("Saved."))).toBeTruthy();
      expect(calls).toEqual([{ key: "sk-test" }]);
      expect(input.value).toBe(""); // write-only — never reflects the saved value back
    });

    it("shows a Clear action once a key is saved, and clears it on click", async () => {
      settingsState.hasAiApiKey = true;
      const calls: string[] = [];
      mockIPC((cmd) => {
        if (cmd === "clear_ai_api_key") {
          calls.push(cmd);
          return null;
        }
        if (cmd === "has_ai_api_key") return false;
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const input = getByLabelText("API key (optional for local servers)") as HTMLInputElement;
      const form = within(input.closest("form")!);

      await fireEvent.click(form.getByText("Clear"));

      await waitFor(() => expect(calls).toEqual(["clear_ai_api_key"]));
    });

    it("saves custom instructions", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        if (cmd === "set_ai_instructions") {
          calls.push(args);
          return { transport: null, instructions: "Use Conventional Commits.", cloudWarningAcknowledged: false };
        }
        throw new Error(`unexpected command ${cmd}`);
      });

      const { getByLabelText } = render(SettingsPanel);
      const textarea = getByLabelText("Custom instructions") as HTMLTextAreaElement;
      const form = within(textarea.closest("form")!);

      await fireEvent.input(textarea, { target: { value: "Use Conventional Commits." } });
      await fireEvent.click(form.getByText("Save"));

      expect(await waitFor(() => form.getByText("Saved."))).toBeTruthy();
      expect(calls).toEqual([{ instructions: "Use Conventional Commits." }]);
    });

    describe("Local AI (managed)", () => {
      type DownloadChannel = { channel: { onmessage: (progress: unknown) => void } };

      const NOT_DOWNLOADED = { modelPresent: false, enginePresent: false, gpuDevice: null };
      const READY = { modelPresent: true, enginePresent: true, gpuDevice: null };

      it("shows a Download button with a size estimate when nothing is downloaded yet", async () => {
        mockIPC((cmd) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });

        expect(await waitFor(() => getByText("Not downloaded yet (~1.1GB)."))).toBeTruthy();
        expect(getByText("Download Local AI Engine")).toBeTruthy();
      });

      it("does not show base URL/model fields for the managed local transport", async () => {
        mockIPC((cmd) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, queryByLabelText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });

        expect(queryByLabelText("Base URL")).toBeNull();
        expect(queryByLabelText("Model")).toBeNull();
      });

      it("defaults the Engine choice to CPU and shows it only under Local AI", async () => {
        mockIPC((cmd) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, queryByLabelText } = render(SettingsPanel);
        expect(queryByLabelText("Engine")).toBeNull();

        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });

        expect((getByLabelText("Engine") as HTMLSelectElement).value).toBe("cpu");
      });

      it("shows Ready status when the model and engine are already present", async () => {
        mockIPC((cmd) => {
          if (cmd === "get_local_ai_status") return READY;
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });

        expect(await waitFor(() => getByText("Ready", { exact: false }))).toBeTruthy();
      });

      it("re-checks status for the Vulkan variant when Engine is switched, without requiring Save", async () => {
        const requestedVariants: unknown[] = [];
        mockIPC((cmd, args) => {
          if (cmd === "get_local_ai_status") {
            requestedVariants.push((args as { engineVariant: string }).engineVariant);
            return (args as { engineVariant: string }).engineVariant === "vulkan" ? READY : NOT_DOWNLOADED;
          }
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await waitFor(() => expect(requestedVariants).toContain("cpu"));

        await fireEvent.change(getByLabelText("Engine"), { target: { value: "vulkan" } });

        expect(await waitFor(() => getByText("Ready", { exact: false }))).toBeTruthy();
        expect(requestedVariants).toContain("vulkan");
      });

      it("shows a GPU reassurance line when a Vulkan device is detected", async () => {
        mockIPC((cmd, args) => {
          if (cmd === "get_local_ai_status") {
            return (args as { engineVariant: string }).engineVariant === "vulkan"
              ? { ...READY, gpuDevice: "NVIDIA GeForce RTX 3060" }
              : NOT_DOWNLOADED;
          }
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await fireEvent.change(getByLabelText("Engine"), { target: { value: "vulkan" } });

        expect(await waitFor(() => getByText("GPU: NVIDIA GeForce RTX 3060"))).toBeTruthy();
      });

      it("shows a no-GPU warning line when Vulkan is selected but no device is detected", async () => {
        mockIPC((cmd, args) => {
          if (cmd === "get_local_ai_status") {
            return (args as { engineVariant: string }).engineVariant === "vulkan" ? READY : NOT_DOWNLOADED;
          }
          throw new Error(`unexpected command ${cmd} before Save is clicked`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await fireEvent.change(getByLabelText("Engine"), { target: { value: "vulkan" } });

        expect(
          await waitFor(() => getByText("No Vulkan-capable GPU detected — this will run on CPU.")),
        ).toBeTruthy();
      });

      it("shows a progress bar reflecting the latest reported chunk while downloading", async () => {
        mockIPC((cmd, args) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          if (cmd === "download_local_ai") {
            (args as DownloadChannel).channel.onmessage({
              stage: "engine",
              bytesDownloaded: 50,
              bytesTotal: 100,
            });
            return new Promise(() => {}); // never resolves — the download is still in flight
          }
          throw new Error(`unexpected command ${cmd}`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await waitFor(() => getByText("Not downloaded yet (~1.1GB)."));
        await fireEvent.click(getByText("Download Local AI Engine"));

        expect(await waitFor(() => getByText("Engine: 50%"))).toBeTruthy();
      });

      it("shows Ready and refreshes status once a download completes", async () => {
        mockIPC((cmd) => {
          if (cmd === "download_local_ai") return null;
          if (cmd === "get_local_ai_status") return READY;
          throw new Error(`unexpected command ${cmd}`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await fireEvent.click(await waitFor(() => getByText("Download Local AI Engine")));

        expect(await waitFor(() => getByText("Ready", { exact: false }))).toBeTruthy();
      });

      it("cancels an in-progress download", async () => {
        const calls: string[] = [];
        mockIPC((cmd) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          if (cmd === "download_local_ai") {
            return new Promise(() => {}); // never resolves — simulates an in-flight download
          }
          if (cmd === "cancel_local_ai_download") {
            calls.push(cmd);
            return null;
          }
          throw new Error(`unexpected command ${cmd}`);
        });

        const { getByLabelText, getByText } = render(SettingsPanel);
        await fireEvent.change(getByLabelText("Provider"), { target: { value: "managedLocal" } });
        await waitFor(() => getByText("Not downloaded yet (~1.1GB)."));
        await fireEvent.click(getByText("Download Local AI Engine"));
        await fireEvent.click(await waitFor(() => getByText("Cancel")));

        expect(calls).toEqual(["cancel_local_ai_download"]);
      });

      it("saves the drafted engine variant on submit", async () => {
        const calls: unknown[] = [];
        mockIPC((cmd, args) => {
          if (cmd === "get_local_ai_status") return NOT_DOWNLOADED;
          if (cmd === "set_ai_transport") {
            calls.push(args);
            return { transport: null, instructions: "", cloudWarningAcknowledged: false };
          }
          throw new Error(`unexpected command ${cmd}`);
        });

        const { getByLabelText } = render(SettingsPanel);
        const providerSelect = getByLabelText("Provider") as HTMLSelectElement;
        await fireEvent.change(providerSelect, { target: { value: "managedLocal" } });
        await fireEvent.change(getByLabelText("Engine"), { target: { value: "vulkan" } });
        const form = within(providerSelect.closest("form")!);
        await fireEvent.click(form.getByText("Save"));

        await waitFor(() =>
          expect(calls).toEqual([
            { transport: { kind: "managedLocal", engineVariant: "vulkan" } },
          ]),
        );
      });
    });
  });

  describe("when a repo is open", () => {
    it("loads and shows the repo's skip-hooks default and commit template", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            expect(args).toEqual({ repoPath: "/repo" });
            return { defaultSkipHooks: true };
          case "get_commit_template_path":
            return "/home/user/.gitmessage";
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByText, getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });

      expect(await waitFor(() => getByText("This Repository"))).toBeTruthy();
      await waitFor(() =>
        expect((getByLabelText("Skip hooks by default") as HTMLInputElement).checked).toBe(true),
      );
      expect((getByLabelText("Commit message template") as HTMLInputElement).value).toBe(
        "/home/user/.gitmessage",
      );
    });

    it("saves the skip-hooks default as soon as the checkbox is toggled", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "set_repo_default_skip_hooks":
            expect(args).toEqual({ repoPath: "/repo", value: true });
            return { defaultSkipHooks: true };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const checkbox = await waitFor(
        () => getByLabelText("Skip hooks by default") as HTMLInputElement,
      );

      await fireEvent.click(checkbox);

      expect(checkbox.checked).toBe(true);
    });

    it("reverts the checkbox and shows an error toast when the save fails", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "set_repo_default_skip_hooks":
            throw "disk full";
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const checkbox = await waitFor(
        () => getByLabelText("Skip hooks by default") as HTMLInputElement,
      );

      await fireEvent.click(checkbox);

      await waitFor(() => expect(checkbox.checked).toBe(false));
      expect(toastState.toasts.map((t) => t.message)).toContain("disk full");
    });

    it("saves the commit template path, clearing it when the field is emptied", async () => {
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return "/home/user/.gitmessage";
          case "set_commit_template_path":
            expect(args).toEqual({ repoPath: "/repo", path: null });
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText } = render(SettingsPanel, { props: { repoPath: "/repo" } });
      const input = await waitFor(
        () => getByLabelText("Commit message template") as HTMLInputElement,
      );
      await waitFor(() => expect(input.value).toBe("/home/user/.gitmessage"));
      const templateForm = within(input.closest("form")!);

      await fireEvent.input(input, { target: { value: "" } });
      await fireEvent.click(templateForm.getByText("Save"));

      expect(await waitFor(() => templateForm.getByText("Saved.", { exact: false }))).toBeTruthy();
    });

    it("shows the GitFlow setup form and initializes it on submit", async () => {
      const calls: unknown[] = [];
      mockIPC((cmd, args) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "detect_workflow":
            return null;
          case "init_workflow":
            calls.push(args);
            return null;
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { getByLabelText, findByText } = render(SettingsPanel, {
        props: { repoPath: "/repo" },
      });
      const mainInput = await waitFor(() => getByLabelText("Main branch name") as HTMLInputElement);
      const setupForm = within(mainInput.closest("form")!);

      await fireEvent.input(mainInput, { target: { value: "main" } });
      await fireEvent.input(getByLabelText("Develop branch name"), {
        target: { value: "develop" },
      });
      await fireEvent.click(setupForm.getByText("Set up"));

      await waitFor(() =>
        expect(calls).toEqual([
          {
            repoPath: "/repo",
            config: {
              main: "main",
              develop: "develop",
              featurePrefix: "feature/",
              releasePrefix: "release/",
              hotfixPrefix: "hotfix/",
              supportPrefix: null,
              versionTagPrefix: "",
            },
          },
        ]),
      );
      expect(await findByText("Configured", { exact: false })).toBeTruthy();
    });

    it("shows GitFlow's configured status instead of the setup form once initialized", async () => {
      mockIPC((cmd) => {
        switch (cmd) {
          case "get_repo_config":
            return { defaultSkipHooks: false };
          case "get_commit_template_path":
            return null;
          case "detect_workflow":
            return {
              main: "main",
              develop: "develop",
              featurePrefix: "feature/",
              releasePrefix: "release/",
              hotfixPrefix: "hotfix/",
              supportPrefix: null,
              versionTagPrefix: "",
            };
          default:
            throw new Error(`unexpected command ${cmd}`);
        }
      });

      const { findByText, queryByText } = render(SettingsPanel, { props: { repoPath: "/repo" } });

      expect(await findByText("Configured", { exact: false })).toBeTruthy();
      expect(queryByText("Set up GitFlow")).toBeNull();
    });
  });
});
