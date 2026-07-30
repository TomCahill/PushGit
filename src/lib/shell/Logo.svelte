<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // The PushGit mark: two stacked chevrons landing on a filled dot — "push velocity landing
  // on a commit". Stroke-only except the terminal dot, per the brand sheet. Uses currentColor
  // so callers set the mark's color by setting `color` on/around this element.
  let { size = 24 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 52 52" fill="none" aria-hidden="true">
  <path
    d="M12 34 L20 26 L12 18"
    stroke="currentColor"
    stroke-width="4"
    stroke-linecap="round"
    stroke-linejoin="round"
  />
  <path
    d="M22 34 L30 26 L22 18"
    stroke="currentColor"
    stroke-width="4"
    stroke-linecap="round"
    stroke-linejoin="round"
    opacity="0.55"
  />
  <circle cx="38" cy="26" r="5" fill="currentColor" />
</svg>
