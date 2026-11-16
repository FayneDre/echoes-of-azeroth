//! Icon in the notification area (ADR 0018): brings the launcher back while
//! it is hidden during a game, stops the servers, quits. Its texts come from
//! the UI (`set_tray_texts`), which knows the player's language.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// The menu entries, kept to translate them.
pub struct Tray {
    show: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

impl Tray {
    pub fn set_texts(&self, show: &str, stop: &str, quit: &str) -> tauri::Result<()> {
        self.show.set_text(show)?;
        self.stop.set_text(stop)?;
        self.quit.set_text(quit)
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<Tray> {
    let show = MenuItem::with_id(app, "show", "Show RealmKeeper", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop the servers", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &stop, &separator, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("RealmKeeper")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "stop" => crate::commands::stop_all(app),
            "quit" => crate::request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(Tray { show, stop, quit })
}

/// Shows the launcher window again, in front.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hides the launcher window (the tray icon brings it back).
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}
