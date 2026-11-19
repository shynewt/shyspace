//! The tray icon: shyspace keeps running there when its window is closed.

use std::panic::AssertUnwindSafe;

use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const MAIN_WINDOW: &str = "main";

/// Whether the tray icon exists. On Linux it needs the system's
/// appindicator library; without it, closing the window must quit, or the
/// app would be hidden with no way back.
pub struct HasTray(pub bool);

/// Creates the tray icon, or reports why there is none. On Linux the
/// appindicator binding panics (rather than failing) when the system library
/// is missing, so the panic is caught here; this relies on the default
/// `panic = "unwind"`.
pub fn try_install(app: &AppHandle) -> bool {
    match std::panic::catch_unwind(AssertUnwindSafe(|| install(app))) {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!("no tray icon, so closing the window quits: {error}");
            false
        }
        Err(_) => {
            tracing::warn!(
                "no tray icon (the system's appindicator library is missing), so closing the window quits"
            );
            false
        }
    }
}

fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open shyspace", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit shyspace", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("shyspace")
        .tooltip("shyspace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        tracing::warn!("the main window is gone");
        return;
    };
    for result in [window.unminimize(), window.show(), window.set_focus()] {
        if let Err(error) = result {
            tracing::warn!("could not show the window: {error}");
        }
    }
}
