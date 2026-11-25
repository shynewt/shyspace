//! Opening shyspace when the person logs in, a choice of this device. The
//! system's own record is the truth, read each time, since it can be
//! changed there too: Login Items on macOS (Apple's `SMAppService`), the
//! `Run` key and Task Manager's Startup apps on Windows, the XDG autostart
//! entry on Linux (`linux_autostart`). A launch at login starts with no
//! window, in the Dock or the tray, so alerts and the badge work at once.

use crate::CommandResult;

/// Passed by the Windows and Linux entries. macOS's login item takes no
/// arguments; there the launch event says it instead (`macos_launch`).
pub const AT_LOGIN_ARGUMENT: &str = "--autostart";

/// Whether this run was started by logging in.
pub fn was_started_at_login() -> bool {
    #[cfg(target_os = "macos")]
    return crate::macos_launch::was_launched_as_login_item();
    #[cfg(not(target_os = "macos"))]
    has_login_argument(std::env::args())
}

/// Whether `arguments` (the program's name first) mark a launch at login.
pub fn has_login_argument(arguments: impl IntoIterator<Item = String>) -> bool {
    arguments
        .into_iter()
        .skip(1)
        .any(|argument| argument == AT_LOGIN_ARGUMENT)
}

/// Asks the next start to show the window even if it carries the login
/// argument: a restart into an update keeps this run's arguments (on
/// Windows through the installer), and would come back hidden.
pub struct ShowNextStart {
    file: std::path::PathBuf,
}

impl ShowNextStart {
    pub fn in_dir(dir: &std::path::Path) -> ShowNextStart {
        ShowNextStart {
            file: dir.join("show-window-at-next-start"),
        }
    }

    pub fn ask(&self) -> anyhow::Result<()> {
        use anyhow::Context;
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        }
        std::fs::write(&self.file, b"").with_context(|| format!("writing {}", self.file.display()))
    }

    /// Whether the last run asked; asking holds for one start only.
    pub fn take(&self) -> bool {
        std::fs::remove_file(&self.file).is_ok()
    }
}

/// Whether to start with the window hidden: only after a login, and only
/// where the app can still be found, in the Dock (macOS) or a tray icon.
pub fn starts_hidden(is_at_login: bool, is_macos: bool, has_tray: bool) -> bool {
    is_at_login && (is_macos || has_tray)
}

/// Whether the tray icon will be seen. Windows always shows it. On Linux
/// the library loads even where nothing shows its icon (stock GNOME, a
/// sway or niri bar without a tray), so it counts only when a panel says it
/// hosts tray icons; when that can't be told, the window shows.
pub fn is_tray_seen(has_tray: bool) -> bool {
    #[cfg(target_os = "linux")]
    return has_tray && is_seen_by(tray_host::is_registered());
    #[cfg(not(target_os = "linux"))]
    has_tray
}

/// A tray host's answer, None when there was no answer.
#[cfg(any(target_os = "linux", test))]
fn is_seen_by(host_registered: Option<bool>) -> bool {
    host_registered == Some(true)
}

/// The StatusNotifierItem watcher on the session bus, which the panel or
/// bar showing tray icons registers with.
#[cfg(target_os = "linux")]
mod tray_host {
    pub fn is_registered() -> Option<bool> {
        let connection = zbus::blocking::Connection::session().ok()?;
        let watcher = zbus::blocking::Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )
        .ok()?;
        watcher.get_property("IsStatusNotifierHostRegistered").ok()
    }
}

#[tauri::command]
pub fn open_at_login() -> CommandResult<bool> {
    system::is_on().map_err(|error| format!("{error:#}"))
}

/// Turns opening at login on or off, then reads the system's record back:
/// the system can refuse quietly (it was turned off in its own settings),
/// and then says what to do.
#[tauri::command]
pub fn set_open_at_login(is_on: bool) -> CommandResult<()> {
    let set = if is_on {
        system::turn_on()
    } else {
        system::turn_off()
    };
    set.map_err(|error| format!("{error:#}"))?;
    if system::is_on().map_err(|error| format!("{error:#}"))? != is_on {
        return Err(if is_on {
            system::REFUSED_HINT
        } else {
            "shyspace is still set to open at login; check your system's startup settings"
        }
        .into());
    }
    Ok(())
}

/// Keeps an entry that's on pointing at this AppImage, which may have moved
/// since. Only an AppImage does this, so a development build or a second
/// install never takes the entry over. Elsewhere the system tracks the app.
pub fn refresh_at_start() {
    #[cfg(target_os = "linux")]
    if let Err(error) = system::refresh() {
        tracing::warn!("couldn't update the open-at-login entry: {error:#}");
    }
}

#[cfg(target_os = "linux")]
mod system {
    use std::path::PathBuf;

    use anyhow::{Context, Result};

    use super::AT_LOGIN_ARGUMENT;
    use crate::linux_autostart::{Autostart, autostart_dir};

    pub const REFUSED_HINT: &str =
        "Your desktop keeps shyspace from opening at login; check its startup settings";

    fn autostart() -> Result<Autostart> {
        let dir = autostart_dir(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME").map(PathBuf::from),
        )
        .context("couldn't find your home folder")?;
        Ok(Autostart::in_dir(&dir))
    }

    /// The AppImage itself rather than its temporary mount, or the binary.
    fn program() -> Result<PathBuf> {
        match std::env::var_os("APPIMAGE") {
            Some(appimage) => Ok(PathBuf::from(appimage)),
            None => std::env::current_exe().context("finding the app's path"),
        }
    }

    pub fn is_on() -> Result<bool> {
        autostart()?.is_on()
    }

    pub fn turn_on() -> Result<()> {
        autostart()?.turn_on(&program()?, AT_LOGIN_ARGUMENT)
    }

    pub fn turn_off() -> Result<()> {
        autostart()?.turn_off()
    }

    pub fn refresh() -> Result<()> {
        if std::env::var_os("APPIMAGE").is_none() || !is_on()? {
            return Ok(());
        }
        turn_on()
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod system {
    use anyhow::{Context, Result};
    use auto_launch::{AutoLaunch, AutoLaunchBuilder};

    #[cfg(target_os = "macos")]
    pub const REFUSED_HINT: &str =
        "macOS needs your OK: allow shyspace in System Settings › General › Login Items";
    #[cfg(target_os = "windows")]
    pub const REFUSED_HINT: &str =
        "Windows has shyspace turned off in Task Manager › Startup apps; turn it on there";

    fn launcher() -> Result<AutoLaunch> {
        let mut builder = AutoLaunchBuilder::new();
        #[cfg(target_os = "macos")]
        builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::SMAppService);
        #[cfg(target_os = "windows")]
        {
            let program = std::env::current_exe().context("finding the app's path")?;
            let program = program.to_str().context("the app's path isn't valid text")?;
            // Quoted, or Windows may first run a program at a shorter path
            // when the path has a space (as a user name can).
            builder
                .set_app_name("shyspace")
                .set_app_path(&format!("\"{program}\""))
                .set_args(&[super::AT_LOGIN_ARGUMENT])
                .set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);
        }
        builder.build().map_err(|error| match error {
            auto_launch::Error::UnsupportedOS => anyhow::anyhow!("opening at login needs macOS 13 or newer"),
            error => anyhow::Error::new(error),
        })
    }

    pub fn is_on() -> Result<bool> {
        launcher()?
            .is_enabled()
            .context("reading whether shyspace opens at login")
    }

    pub fn turn_on() -> Result<()> {
        launcher()?.enable().context("setting shyspace to open at login")
    }

    pub fn turn_off() -> Result<()> {
        launcher()?
            .disable()
            .context("stopping shyspace from opening at login")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(list: &[&str]) -> Vec<String> {
        list.iter().map(|argument| (*argument).to_owned()).collect()
    }

    #[test]
    fn only_the_login_argument_marks_a_login_launch() {
        assert!(has_login_argument(arguments(&[
            "/usr/bin/shyspace",
            "--autostart"
        ])));
        assert!(!has_login_argument(arguments(&["/usr/bin/shyspace"])));
        assert!(
            !has_login_argument(arguments(&["--autostart"])),
            "the program's own name isn't an argument"
        );
        assert!(!has_login_argument(arguments(&[
            "/usr/bin/shyspace",
            "--autostart=no"
        ])));
    }

    #[test]
    fn a_start_after_an_update_shows_the_window_once() {
        let dir = std::env::temp_dir().join(format!("shyspace-show-next-start-{}", std::process::id()));
        let marker = ShowNextStart::in_dir(&dir);
        assert!(!marker.take(), "nothing asked yet");

        marker.ask().unwrap();

        assert!(marker.take());
        assert!(!marker.take(), "only once");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_login_launch_hides_only_where_the_app_can_still_be_found() {
        assert!(starts_hidden(true, true, false), "the Dock");
        assert!(starts_hidden(true, false, true), "the tray");
        assert!(!starts_hidden(true, false, false), "no tray: show the window");
        assert!(!starts_hidden(false, true, true), "an ordinary launch shows it");
    }

    #[test]
    fn a_tray_counts_only_when_a_panel_says_it_hosts_it() {
        assert!(is_seen_by(Some(true)));
        assert!(!is_seen_by(Some(false)), "a watcher with no panel");
        assert!(!is_seen_by(None), "no watcher, or no answer: show the window");
    }
}
