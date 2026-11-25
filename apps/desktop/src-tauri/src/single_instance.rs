//! One shyspace per person: opening it again hands over to the running app,
//! which shows its window, and the new process exits. A second launch at
//! login stays quiet. Through Tauri's single-instance plugin, which must be
//! the first plugin registered.

use tauri::{AppHandle, Wry, plugin::TauriPlugin};

use crate::{open_at_login, tray};

/// Which lock this run takes, if any.
#[derive(Debug, PartialEq, Eq)]
enum InstanceLock {
    /// The installed app's.
    App,
    /// A development build's own (Linux's D-Bus name can be chosen), so it
    /// and an installed AppImage don't hand over to each other.
    DevelopmentBuild,
    /// None: `SHY_DIR` runs several members side by side on purpose (each
    /// folder's workspace lock still stops two runs of one), and on macOS
    /// and Windows a development build can't take a lock of its own.
    None,
}

fn instance_lock(has_shy_dir: bool, is_development_build: bool, is_linux: bool) -> InstanceLock {
    match (has_shy_dir, is_development_build, is_linux) {
        (true, _, _) => InstanceLock::None,
        (false, false, _) => InstanceLock::App,
        (false, true, true) => InstanceLock::DevelopmentBuild,
        (false, true, false) => InstanceLock::None,
    }
}

/// The plugin for this run, or None when it runs without one.
pub fn plugin() -> Option<TauriPlugin<Wry>> {
    let lock = instance_lock(
        std::env::var_os("SHY_DIR").is_some(),
        cfg!(debug_assertions),
        cfg!(target_os = "linux"),
    );
    let builder = tauri_plugin_single_instance::Builder::new().callback(on_second_launch);
    match lock {
        InstanceLock::App => Some(builder.build()),
        InstanceLock::DevelopmentBuild => Some(builder.dbus_id("dev.shynewt.shyspace.development").build()),
        InstanceLock::None => None,
    }
}

fn on_second_launch(app: &AppHandle, arguments: Vec<String>, _folder: String) {
    if shows_window_for(arguments) {
        tray::show_main_window(app);
    }
}

/// A launch at login while shyspace already runs changes nothing.
fn shows_window_for(arguments: Vec<String>) -> bool {
    !open_at_login::has_login_argument(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_it_again_shows_the_window_but_a_login_launch_doesnt() {
        assert!(shows_window_for(vec!["/usr/bin/shyspace".into()]));
        assert!(!shows_window_for(vec![
            "/usr/bin/shyspace".into(),
            "--autostart".into()
        ]));
    }

    #[test]
    fn the_installed_app_takes_the_lock_and_development_builds_keep_apart() {
        assert_eq!(instance_lock(false, false, true), InstanceLock::App);
        assert_eq!(instance_lock(false, false, false), InstanceLock::App);
        assert_eq!(instance_lock(false, true, true), InstanceLock::DevelopmentBuild);
        assert_eq!(instance_lock(false, true, false), InstanceLock::None);
    }

    #[test]
    fn several_members_side_by_side_take_no_lock() {
        assert_eq!(instance_lock(true, false, true), InstanceLock::None);
        assert_eq!(instance_lock(true, true, false), InstanceLock::None);
    }
}
