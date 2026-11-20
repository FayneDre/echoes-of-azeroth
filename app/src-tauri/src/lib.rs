//! Tauri shell. Keep this crate thin: business logic belongs in `crates/*`
//! (`rk-launcher`), this layer only adapts it to commands and events.
//! Unsafe code is refused here, except in `os` (Win32 calls, audited).
#![deny(unsafe_code)]

mod commands;
mod os;
mod tray;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_window_state::StateFlags;

/// Smallest window the pages are laid out for (`tauri.conf.json`).
const MIN_WIDTH: f64 = 1024.0;
const MIN_HEIGHT: f64 = 760.0;

pub fn run() {
    // Opt-in crash reports (ADR 0015): written locally, offered at next start.
    rk_launcher::crash::install_hook(&rk_launcher::app_dir(), env!("CARGO_PKG_VERSION"));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // Size and place as the player left them (the first start is centred).
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::POSITION | StateFlags::SIZE | StateFlags::MAXIMIZED)
                .build(),
        )
        .setup(|app| {
            // The launcher update feed exists only in release builds made by
            // CI with the updater public key (ADR 0015).
            if commands::has_update_feed(app.handle()) {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            let state = commands::AppState::open(app.handle());
            app.manage(state);
            app.manage(tray::create(app.handle())?);
            if let Some(window) = app.get_webview_window("main") {
                let _ = fit_to_screen(&window);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window with servers running: stop them cleanly
            // first (worldserver saves the world and logs the bots out).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if app.state::<commands::AppState>().running_ids().is_empty() {
                    return;
                }
                api.prevent_close();
                request_exit(app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::catalog_overview,
            commands::launcher_overview,
            commands::register_client,
            commands::find_clients,
            commands::install_space,
            commands::install_instance,
            commands::update_instance,
            commands::server_update,
            commands::repair_instance,
            commands::resume_instance,
            commands::rebuilds_for,
            commands::set_modules,
            commands::instance_options,
            commands::set_options,
            commands::cancel_operation,
            commands::create_account,
            commands::list_accounts,
            commands::set_gm_level,
            commands::online_characters,
            commands::teleport_locations,
            commands::run_console_action,
            commands::party_offered,
            commands::party_add,
            commands::party_remove,
            commands::party_members,
            commands::start_instance,
            commands::stop_instance,
            commands::console_command,
            commands::launch_game,
            commands::realm_settings,
            commands::uninstall_instance,
            commands::data_location,
            commands::set_data_location,
            commands::crash_reports,
            commands::check_app_update,
            commands::install_app_update,
            commands::dismiss_crash_report,
            commands::list_backups,
            commands::backup_now,
            commands::delete_backup,
            commands::firewall_status,
            commands::allow_firewall,
            commands::remove_firewall,
            commands::restore_backup,
            commands::duplicate_instance,
            commands::export_instance,
            commands::read_export,
            commands::import_instance,
            commands::rename_instance,
            commands::open_backups_folder,
            commands::join_lan,
            commands::diagnostic_report,
            commands::preferences,
            commands::set_preferences,
            commands::set_tray_texts,
        ])
        .build(tauri::generate_context!())
        .expect("error while building RealmKeeper")
        .run(|app, event| {
            // The last moment before the process ends, including when
            // Windows shuts down or logs off (tao exits right after): the
            // servers still running stop cleanly first (ADR 0020).
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<commands::AppState>();
                if !state.running_ids().is_empty() {
                    tauri::async_runtime::block_on(state.shutdown());
                }
            }
        });
}

/// Quits the launcher; servers still running are stopped cleanly first
/// (the window shows why it waits).
pub fn request_exit(app: &AppHandle) {
    if app.state::<commands::AppState>().running_ids().is_empty() {
        app.exit(0);
        return;
    }
    tray::show_main(app);
    let _ = app.emit("closing", ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<commands::AppState>().shutdown().await;
        app.exit(0);
    });
}

/// Keeps the window on the screen it opens on: a laptop screen smaller than
/// the default (or the minimum) size gets a window that fits its work area.
fn fit_to_screen(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let (max_w, max_h) = (area.size.width, area.size.height);
    let min =
        tauri::LogicalSize::new(MIN_WIDTH, MIN_HEIGHT).to_physical::<u32>(monitor.scale_factor());
    if min.width > max_w || min.height > max_h {
        window.set_min_size(Some(tauri::PhysicalSize::new(
            min.width.min(max_w),
            min.height.min(max_h),
        )))?;
    }
    if window.is_maximized()? {
        return Ok(());
    }
    let size = window.outer_size()?;
    if size.width > max_w || size.height > max_h {
        window.set_size(tauri::PhysicalSize::new(
            size.width.min(max_w),
            size.height.min(max_h),
        ))?;
        window.set_position(area.position)?;
    }
    Ok(())
}
