//! Tauri shell. Keep this crate thin: business logic belongs in `crates/*`,
//! this layer only adapts it to commands and events for the UI.

mod commands;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::catalog_overview])
        .run(tauri::generate_context!())
        .expect("error while running Echoes of Azeroth");
}
