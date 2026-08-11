// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

// Shared scaffolding behind an ActionRail-hosted panel's data lifecycle
// (BranchSidebar/TagsPanel/StashPanel/WorkflowPanel/MaintenancePanel/UndoRedoControls): a
// generation-guarded reload that discards a response once a newer reload has started, and a
// busy-guarded action runner that re-reloads and reports success once the wrapped action
// completes. Each caller still owns *what* to fetch/set into its own state and *how* to report an
// error — only this bookkeeping is shared. RemotePanel deliberately doesn't use this: its busy
// flag is also driven by an independent auto-fetch timer and paired with progress-stream state,
// a genuinely different shape from the guard-then-run pattern this covers.

export interface Reloadable {
  readonly busy: boolean;
  reload(path: string, refreshKey?: number): Promise<void>;
  runAction(
    path: string,
    fn: () => Promise<unknown>,
    onError: (message: string) => void,
    onSuccess?: () => void,
  ): Promise<void>;
}

export function createReloadable(
  load: (path: string, isStale: () => boolean) => Promise<void>,
): Reloadable {
  let generation = 0;
  let busy = $state(false);

  async function reload(path: string, _refreshKey?: number): Promise<void> {
    const myGeneration = ++generation;
    await load(path, () => myGeneration !== generation);
  }

  async function runAction(
    path: string,
    fn: () => Promise<unknown>,
    onError: (message: string) => void,
    onSuccess?: () => void,
  ): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await fn();
      await reload(path);
      onSuccess?.();
    } catch (err) {
      onError(String(err));
    } finally {
      busy = false;
    }
  }

  return {
    get busy() {
      return busy;
    },
    reload,
    runAction,
  };
}
