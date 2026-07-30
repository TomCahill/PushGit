<!--
Copyright (C) 2026 Tom Cahill
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<script lang="ts">
  // Test-only harness: `Popover.svelte.test.ts` needs a `<Popover>` instance with real
  // snippet content, which a plain `.test.ts` file can't express — components with slotted
  // markup need an actual `.svelte` host, same reasoning as `HunkDiff.svelte.test.ts` using
  // `testFixtures.ts` builders rather than inlining fixture objects.
  import Popover from "./Popover.svelte";

  let { portal = false }: { portal?: boolean } = $props();
</script>

<div data-testid="outside">outside content</div>
<Popover {portal}>
  {#snippet trigger()}
    Open me
  {/snippet}
  {#snippet children()}
    <p>panel content</p>
  {/snippet}
</Popover>
