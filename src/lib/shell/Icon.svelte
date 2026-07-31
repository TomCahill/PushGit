<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // A small, shared line-icon set (Feather/Lucide-style: 24x24 viewBox, 2px stroke,
  // round caps/joins) so every toolbar/rail trigger draws from one consistent visual
  // language instead of ad-hoc SVGs scattered per component.
  export type IconName =
    | "chevron-down"
    | "branch"
    | "tag"
    | "archive"
    | "refresh-cw"
    | "arrow-down"
    | "arrow-up"
    | "folder"
    | "x"
    | "plus"
    | "minus"
    | "edit-2"
    | "arrow-right"
    | "alert-triangle"
    | "help-circle"
    | "clock"
    | "settings"
    | "copy"
    | "check"
    | "undo"
    | "redo"
    | "wrench"
    | "bell"
    | "sparkles"
    | "square"
    | "download";

  let { name, size = 14 }: { name: IconName; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  class="icon"
>
  {#if name === "chevron-down"}
    <path d="M6 9l6 6 6-6" />
  {:else if name === "branch"}
    <circle cx="6" cy="6" r="2.5" />
    <circle cx="6" cy="18" r="2.5" />
    <circle cx="18" cy="6" r="2.5" />
    <path d="M6 8.5v7" />
    <path d="M18 8.5a6 6 0 0 1-6 6h-3" />
  {:else if name === "tag"}
    <path
      d="M12.59 2.59a2 2 0 0 0-1.42-.59H4a2 2 0 0 0-2 2v7.17a2 2 0 0 0 .59 1.42l8.83 8.83a2 2 0 0 0 2.83 0l7.17-7.17a2 2 0 0 0 0-2.83z"
    />
    <circle cx="7.5" cy="7.5" r="1.25" fill="currentColor" stroke="none" />
  {:else if name === "archive"}
    <rect x="2.5" y="4" width="19" height="4.5" rx="1" />
    <path d="M4 8.5v9.5a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8.5" />
    <path d="M9.5 12.5h5" />
  {:else if name === "refresh-cw"}
    <path d="M20 11a8 8 0 0 0-14.9-3.5M4 5v4.5h4.5" />
    <path d="M4 13a8 8 0 0 0 14.9 3.5M20 19v-4.5h-4.5" />
  {:else if name === "arrow-down"}
    <path d="M12 4v14" />
    <path d="M6 12l6 6 6-6" />
  {:else if name === "arrow-up"}
    <path d="M12 20V6" />
    <path d="M6 12l6-6 6 6" />
  {:else if name === "folder"}
    <path
      d="M3 6a1.5 1.5 0 0 1 1.5-1.5h4.4a1.5 1.5 0 0 1 1.2.6l1.2 1.6a1.5 1.5 0 0 0 1.2.6H19.5A1.5 1.5 0 0 1 21 8.8V18a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z"
    />
  {:else if name === "x"}
    <path d="M18 6L6 18" />
    <path d="M6 6l12 12" />
  {:else if name === "plus"}
    <path d="M12 5v14" />
    <path d="M5 12h14" />
  {:else if name === "minus"}
    <path d="M5 12h14" />
  {:else if name === "edit-2"}
    <path d="M17 3a2.83 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5z" />
  {:else if name === "arrow-right"}
    <path d="M5 12h14" />
    <path d="M12 5l7 7-7 7" />
  {:else if name === "alert-triangle"}
    <path
      d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"
    />
    <path d="M12 9v4" />
    <path d="M12 17h.01" />
  {:else if name === "help-circle"}
    <circle cx="12" cy="12" r="9" />
    <path d="M9.5 9a2.5 2.5 0 0 1 4.9.8c0 1.7-2.4 2-2.4 3.7" />
    <path d="M12 17h.01" />
  {:else if name === "clock"}
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3.5 2" />
  {:else if name === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
    />
  {:else if name === "copy"}
    <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  {:else if name === "check"}
    <path d="M20 6L9 17l-4-4" />
  {:else if name === "undo"}
    <path d="M9 14L4 9l5-5" />
    <path d="M20 20v-7a4 4 0 0 0-4-4H4" />
  {:else if name === "redo"}
    <path d="M15 14l5-5-5-5" />
    <path d="M4 20v-7a4 4 0 0 1 4-4h12" />
  {:else if name === "wrench"}
    <path d="M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4l-2.8 2.8-2-2z" />
  {:else if name === "bell"}
    <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
    <path d="M13.73 21a2 2 0 0 1-3.46 0" />
  {:else if name === "sparkles"}
    <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6z" />
    <path d="M19 3v4" />
    <path d="M17 5h4" />
  {:else if name === "square"}
    <rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor" stroke="none" />
  {:else if name === "download"}
    <path d="M12 3v12" />
    <path d="M7 10l5 5 5-5" />
    <path d="M4 19h16" />
  {/if}
</svg>

<style>
  .icon {
    flex-shrink: 0;
    display: block;
  }
</style>
