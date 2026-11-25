//! Whether the system lets shyspace show notifications. Only macOS asks
//! (`notify_macos`); elsewhere, and in development builds on macOS, the
//! answer is always "allowed".

use tauri::AppHandle;

use crate::CommandResult;
use crate::alert_actions::Permission;

#[tauri::command]
pub async fn notification_permission() -> CommandResult<Permission> {
    #[cfg(target_os = "macos")]
    if crate::notify_macos::is_available() {
        return crate::notify_macos::permission().await;
    }
    Ok(Permission::Allowed)
}

/// Asks for permission if it hasn't been asked yet; macOS shows its prompt
/// only once, so afterwards this only reads the answer.
#[tauri::command]
pub async fn request_notification_permission() -> CommandResult<Permission> {
    #[cfg(target_os = "macos")]
    if crate::notify_macos::is_available() {
        return crate::notify_macos::request_permission().await;
    }
    Ok(Permission::Allowed)
}

/// Opens System Settings at shyspace's notification settings.
#[tauri::command]
pub fn open_notification_settings(app: AppHandle) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_opener::OpenerExt;
        const PANE: &str = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
        let app_pane = format!("{PANE}?id={}", app.config().identifier);
        let opener = app.opener();
        return opener
            .open_url(app_pane, None::<&str>)
            .or_else(|_| opener.open_url(PANE, None::<&str>))
            .map_err(|error| error.to_string());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("only macOS asks for permission to notify".into())
    }
}
