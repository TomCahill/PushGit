// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{Emitter, Manager};

mod ai;
mod blame;
mod branch;
mod cherry_pick_range;
mod commands;
mod config;
mod diff;
pub mod error;
mod graph;
mod hooks;
mod interactive_rebase;
mod maintenance;
mod remote;
pub mod repo;
mod stage;
mod stash;
mod state;
#[cfg(test)]
mod test_support;
mod undo;
mod watcher;
mod workflow;

/// Registers the WebdriverIO E2E-testing plugins when built with
/// `--features e2e` (see `npm run test:e2e:build`) — a no-op passthrough otherwise, so a
/// normal `tauri dev`/`tauri build` never links the WebDriver execute/mock backdoor or the
/// embedded WebDriver HTTP server.
#[cfg(feature = "e2e")]
fn register_e2e_plugins<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init())
}

#[cfg(not(feature = "e2e"))]
fn register_e2e_plugins<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder
}

/// Reads the optional `repo-path` positional CLI arg (`tauri-plugin-cli`)
/// and stashes it in `AppState` for
/// `commands::get_startup_repo_path` to hand back to the frontend once. A missing/unparseable
/// CLI invocation just leaves it `None` — never fatal, matching `config`'s own
/// degrade-on-failure philosophy.
fn read_startup_repo_path<R: tauri::Runtime>(app: &tauri::App<R>) -> Option<String> {
    use tauri_plugin_cli::CliExt;

    let matches = app.cli().matches().ok()?;
    let arg = matches.args.get("repo-path")?;
    let path = arg.value.as_str()?;
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Native File/View/Help menu — kept deliberately
/// modest: only items with a zero-plumbing existing entry point at `+page.svelte`'s top level
/// (`pickRepositoryFolder`/`openRepo`, `openCommandPalette`, the `viewMode` toggle, the
/// `showSettings`/`showAbout` overlay flags). Undo/Redo and Fetch/Pull/Push are real actions but
/// live as local component state today, not a shared singleton, so wiring them here is deferred
/// (see the plan's Phase E). Each item's id is emitted verbatim as a `"menu-action"` event for
/// `+page.svelte` to route, except `"exit-program"`: handled directly in `on_menu_event` via
/// `app.exit(0)` rather than round-tripped to the frontend, both because quitting shouldn't
/// depend on a frontend listener being alive and because `PredefinedMenuItem::quit`'s custom
/// label isn't reliably honored by the GTK/Linux menu backend — an explicit item sidesteps that.
fn build_menu<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
) -> tauri::Result<tauri::menu::Menu<R>> {
    let open_repository = MenuItemBuilder::with_id("open-repository", "Open Repository…")
        .accelerator("CmdOrCtrl+O")
        .build(handle)?;
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(handle)?;
    let exit_program = MenuItemBuilder::with_id("exit-program", "Exit Program").build(handle)?;
    let file_menu = SubmenuBuilder::new(handle, "File")
        .item(&open_repository)
        .item(&settings)
        .separator()
        .item(&exit_program)
        .build()?;

    let command_palette = MenuItemBuilder::with_id("command-palette", "Command Palette")
        .accelerator("CmdOrCtrl+P")
        .build(handle)?;
    let toggle_diff_view =
        MenuItemBuilder::with_id("toggle-diff-view", "Toggle Diff View").build(handle)?;
    let view_menu = SubmenuBuilder::new(handle, "View")
        .item(&command_palette)
        .item(&toggle_diff_view)
        .build()?;

    let about = MenuItemBuilder::with_id("about", "About").build(handle)?;
    let help_menu = SubmenuBuilder::new(handle, "Help").item(&about).build()?;

    MenuBuilder::new(handle)
        .item(&file_menu)
        .item(&view_menu)
        .item(&help_menu)
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    register_e2e_plugins(
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .plugin(tauri_plugin_window_state::Builder::default().build())
            .plugin(tauri_plugin_cli::init())
            .plugin(tauri_plugin_notification::init())
            .plugin(tauri_plugin_opener::init()),
    )
    .manage(state::AppState::default())
    .menu(build_menu)
    .on_menu_event(|app, event| {
        if event.id().0 == "exit-program" {
            app.exit(0);
        } else {
            let _ = app.emit("menu-action", event.id().0.clone());
        }
    })
    .setup(|app| {
        if let Some(path) = read_startup_repo_path(app) {
            *app.state::<state::AppState>()
                .startup_repo_path
                .lock()
                .expect("startup_repo_path mutex poisoned") = Some(path);
        }
        Ok(())
    })
    .invoke_handler(tauri::generate_handler![
        commands::open_repository,
        commands::graph_open,
        commands::graph_page,
        commands::graph_close,
        commands::diff_unstaged,
        commands::diff_staged,
        commands::diff_commit,
        commands::diff_between_commits,
        commands::diff_commit_to_workdir,
        commands::blame_file,
        commands::file_history,
        commands::stage_file,
        commands::unstage_file,
        commands::stage_hunk,
        commands::unstage_hunk,
        commands::stage_lines,
        commands::unstage_lines,
        commands::commit,
        commands::head_commit_message,
        commands::commit_message_template,
        commands::list_branches,
        commands::create_branch,
        commands::checkout_branch,
        commands::checkout_commit,
        commands::checkout_remote_branch,
        commands::rename_branch,
        commands::delete_branch,
        commands::merge_branch,
        commands::abort_merge,
        commands::cherry_pick_commit,
        commands::abort_cherry_pick,
        commands::is_multi_cherry_pick_in_progress,
        commands::reset_to,
        commands::cherry_pick_range,
        commands::continue_cherry_pick_range,
        commands::abort_cherry_pick_range,
        commands::rebase_branch,
        commands::abort_rebase,
        commands::continue_rebase,
        commands::list_conflicts,
        commands::repository_state,
        commands::resolve_conflict,
        commands::conflict_sides,
        commands::write_resolved_conflict,
        commands::resolve_conflict_as_deleted,
        commands::discard_file_changes,
        commands::undo_last_operation,
        commands::redo_last_operation,
        commands::undo_redo_status,
        commands::list_rebase_commits,
        commands::is_interactive_rebase_in_progress,
        commands::start_interactive_rebase,
        commands::continue_interactive_rebase,
        commands::abort_interactive_rebase,
        commands::list_tags,
        commands::create_tag,
        commands::delete_tag,
        commands::move_tag,
        commands::rename_tag,
        commands::detect_workflow,
        commands::init_workflow,
        commands::start_workflow_branch,
        commands::finish_workflow_branch,
        commands::check_git_version,
        commands::repo_health,
        commands::run_gc,
        commands::fetch,
        commands::pull,
        commands::push,
        commands::cancel_remote_operation,
        commands::clone_repository,
        commands::create_stash,
        commands::create_stash_for_paths,
        commands::rename_stash,
        commands::list_stashes,
        commands::apply_stash,
        commands::pop_stash,
        commands::drop_stash,
        commands::start_repo_watcher,
        commands::stop_repo_watcher,
        commands::get_app_config,
        commands::set_max_commits_rendered,
        commands::set_reduce_motion,
        commands::set_auto_fetch_enabled,
        commands::set_auto_fetch_interval_minutes,
        commands::set_show_hook_output_always,
        commands::get_repo_config,
        commands::set_repo_default_skip_hooks,
        commands::get_commit_template_path,
        commands::set_commit_template_path,
        commands::get_startup_repo_path,
        commands::get_ai_settings,
        commands::set_ai_transport,
        commands::set_ai_instructions,
        commands::set_ai_api_key,
        commands::clear_ai_api_key,
        commands::has_ai_api_key,
        commands::acknowledge_ai_cloud_warning,
        commands::generate_commit_message,
        commands::cancel_ai_generation,
        commands::get_local_ai_status,
        commands::download_local_ai,
        commands::cancel_local_ai_download,
    ])
    .build(tauri::generate_context!())
    .expect("error while running tauri application")
    .run(|app_handle, event| {
        // The only long-lived child process this app manages — every other subprocess (`git`)
        // is spawned and awaited to completion within a single command call, so this is the
        // first thing that needs cleanup on exit rather than living for the app's lifetime.
        if let tauri::RunEvent::ExitRequested { .. } = event {
            if let Ok(mut engine) = app_handle
                .state::<state::AppState>()
                .local_engine
                .try_lock()
            {
                if let Some(mut handle) = engine.take() {
                    handle.kill();
                }
            }
        }
    });
}
