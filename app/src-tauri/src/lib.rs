//! Tauri shell. Keep this crate thin: business logic belongs in `crates/*`
//! (`eoa-launcher`), this layer only adapts it to commands and events.

mod commands;

use tauri::{Emitter, Manager};

pub fn run() {
    // Opt-in crash reports (ADR 0015): written locally, offered at next start.
    eoa_launcher::crash::install_hook(&eoa_launcher::default_root(), env!("CARGO_PKG_VERSION"));
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The launcher update feed exists only in release builds made by
            // CI with the updater public key (ADR 0015).
            if commands::has_update_feed(app.handle()) {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            let state = commands::AppState::open(app.handle());
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window with servers running: stop them cleanly
            // first (worldserver saves the world and logs the bots out).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let state = app.state::<commands::AppState>();
                if state.running_ids().is_empty() {
                    return;
                }
                api.prevent_close();
                let _ = app.emit("closing", ());
                tauri::async_runtime::spawn(async move {
                    app.state::<commands::AppState>().shutdown().await;
                    app.exit(0);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::catalog_overview,
            commands::launcher_overview,
            commands::register_client,
            commands::install_instance,
            commands::update_instance,
            commands::repair_instance,
            commands::resume_instance,
            commands::rebuilds_for,
            commands::set_modules,
            commands::instance_options,
            commands::set_options,
            commands::cancel_operation,
            commands::create_account,
            commands::start_instance,
            commands::stop_instance,
            commands::console_command,
            commands::crash_reports,
            commands::check_app_update,
            commands::install_app_update,
            commands::dismiss_crash_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Echoes of Azeroth");
}
