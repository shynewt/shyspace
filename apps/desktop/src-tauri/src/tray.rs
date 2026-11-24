//! The app's icon outside its window: a tray icon on Linux and Windows,
//! where shyspace keeps running when its window is closed, and on macOS an
//! optional menu bar item beside the Dock icon. Also the unread count shown
//! on them.

use std::{
    panic::AssertUnwindSafe,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

use shy_core::workspaces::AppIcon;
use tauri::{
    AppHandle, Manager, Theme,
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::AppState;

pub const MAIN_WINDOW: &str = "main";

/// Whether the tray icon exists. On Linux it needs the system's
/// appindicator library; without it, closing the window must quit, or the
/// app would be hidden with no way back. On macOS it follows the menu bar
/// setting.
#[derive(Default)]
pub struct HasTray(AtomicBool);

impl HasTray {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn set(&self, has_tray: bool) {
        self.0.store(has_tray, Ordering::SeqCst);
    }
}

/// How many things need you now, so the icon can be redrawn with it.
#[derive(Default)]
pub struct Attention(AtomicU32);

impl Attention {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }
}

const IS_MACOS: bool = cfg!(target_os = "macos");
const GLYPH_PIXELS: u32 = 64;

const TRAY_ID: &str = "shyspace";

/// Creates the tray icon, or reports why there is none. On Linux the
/// appindicator binding panics (rather than failing) when the system library
/// is missing, so the panic is caught here; this relies on the default
/// `panic = "unwind"`.
fn try_install(app: &AppHandle) -> bool {
    let is_installed = match std::panic::catch_unwind(AssertUnwindSafe(|| install(app))) {
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
    };
    app.state::<HasTray>().set(is_installed);
    is_installed
}

/// Puts the icon up at start: always on Linux and Windows, on macOS only if
/// the menu bar item is turned on.
pub fn install_at_start(app: &AppHandle) {
    match app_icon(app) {
        Ok(icon) if IS_MACOS && !icon.is_in_menu_bar => {}
        Ok(_) => {
            try_install(app);
        }
        Err(error) => tracing::warn!("couldn't read how the app's icon shows: {error:#}"),
    }
}

/// Turns the macOS menu bar item on or off at once.
pub fn show_in_menu_bar(app: &AppHandle, is_shown: bool) {
    if !IS_MACOS {
        return;
    }
    if is_shown && app.tray_by_id(TRAY_ID).is_none() {
        try_install(app);
    } else if !is_shown {
        app.remove_tray_by_id(TRAY_ID);
        app.state::<HasTray>().set(false);
    }
}

fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open shyspace", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit shyspace", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
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
    if let Some(icon) = current_image(app) {
        tray = tray.icon(icon).icon_as_template(IS_MACOS);
    }
    tray.build(app)?;
    Ok(())
}

/// Draws the icon again: after the unread count, a setting or the theme
/// changed.
pub fn redraw(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Err(error) = tray.set_icon(current_image(app)) {
        tracing::warn!("could not redraw the tray icon: {error}");
    }
    if let Err(error) = tray.set_icon_as_template(IS_MACOS) {
        tracing::warn!("could not redraw the tray icon: {error}");
    }
}

fn current_image(app: &AppHandle) -> Option<Image<'static>> {
    let icon = match app_icon(app) {
        Ok(icon) => icon,
        Err(error) => {
            tracing::warn!("couldn't read how the app's icon shows: {error:#}");
            return app.default_window_icon().map(|icon| icon.clone().to_owned());
        }
    };
    let is_unread = app.state::<Attention>().count() > 0;
    match glyph(IS_MACOS, icon, is_dark_panel(app), is_unread) {
        Glyph::AppIcon => app.default_window_icon().map(|icon| icon.clone().to_owned()),
        Glyph::Planet { colour, is_unread } => Some(Image::new(
            planet_pixels(colour, is_unread),
            GLYPH_PIXELS,
            GLYPH_PIXELS,
        )),
    }
}

fn app_icon(app: &AppHandle) -> anyhow::Result<AppIcon> {
    app.state::<AppState>().workspaces.app_icon()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    Black,
    White,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    AppIcon,
    Planet { colour: Colour, is_unread: bool },
}

/// Which icon to show. The macOS menu bar takes a black template image,
/// which macOS tints for light and dark; elsewhere the planet is drawn in
/// the colour that stands out on the panel, when asked for.
fn glyph(is_macos: bool, icon: AppIcon, is_dark_panel: bool, is_unread: bool) -> Glyph {
    let colour = match (is_macos, is_dark_panel) {
        (true, _) | (false, false) => Colour::Black,
        (false, true) => Colour::White,
    };
    if is_macos || icon.is_tray_monochrome {
        Glyph::Planet { colour, is_unread }
    } else {
        Glyph::AppIcon
    }
}

fn planet_pixels(colour: Colour, is_unread: bool) -> &'static [u8] {
    match (colour, is_unread) {
        (Colour::Black, false) => include_bytes!("../icons/tray/planet-black.rgba"),
        (Colour::Black, true) => include_bytes!("../icons/tray/planet-black-unread.rgba"),
        (Colour::White, false) => include_bytes!("../icons/tray/planet-white.rgba"),
        (Colour::White, true) => include_bytes!("../icons/tray/planet-white-unread.rgba"),
    }
}

/// Whether the panel holding the tray icon is dark. GNOME's top bar is dark
/// whatever the theme; elsewhere the panel follows the system's light or
/// dark preference, as the window's theme reports it.
fn is_dark_panel(app: &AppHandle) -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let theme = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.theme().ok());
    is_dark_panel_on(&desktop, theme)
}

fn is_dark_panel_on(desktop: &str, theme: Option<Theme>) -> bool {
    desktop.split(':').any(|name| name.eq_ignore_ascii_case("gnome")) || theme == Some(Theme::Dark)
}

/// Brings the app back after the updater's exit cleanup ran but its
/// installer didn't start: that cleanup removes the tray icon and, on
/// Windows, hides the windows.
pub fn restore_after_cancelled_exit(app: &AppHandle) {
    if app.state::<HasTray>().get() && app.tray_by_id(TRAY_ID).is_none() {
        try_install(app);
    }
    show_main_window(app);
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

/// Shows how many things need you outside the window: in its title, on the
/// Dock or taskbar icon (unless turned off), and on the tray icon: a dot on
/// the planet, and on Linux and Windows the count beside it. Where a
/// platform has no badge or tray, that part is skipped; the title always
/// changes.
pub fn show_attention(app: &AppHandle, count: u32) -> tauri::Result<()> {
    app.state::<Attention>().0.store(count, Ordering::SeqCst);
    let window = app
        .get_webview_window(MAIN_WINDOW)
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let title = match count {
        0 => "shyspace".to_owned(),
        _ => format!("shyspace ({count})"),
    };
    window.set_title(&title)?;
    let shows_count = app_icon(app).map_or(true, |icon| icon.shows_unread_count);
    let badge = (count > 0 && shows_count).then_some(i64::from(count));
    if let Err(error) = window.set_badge_count(badge) {
        tracing::debug!("no taskbar badge on this system: {error}");
    }
    redraw(app);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let label = (count > 0 && !IS_MACOS).then(|| count.to_string());
        let tooltip = match count {
            0 => "shyspace".to_owned(),
            1 => "shyspace: 1 thing needs you".to_owned(),
            _ => format!("shyspace: {count} things need you"),
        };
        tray.set_title(label)?;
        tray.set_tooltip(Some(tooltip))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: AppIcon = AppIcon {
        is_in_menu_bar: true,
        shows_unread_count: true,
        is_tray_monochrome: false,
    };
    const MONOCHROME: AppIcon = AppIcon {
        is_tray_monochrome: true,
        ..QUIET
    };

    #[test]
    fn the_macos_menu_bar_always_takes_the_template_planet() {
        for icon in [QUIET, MONOCHROME] {
            for is_dark_panel in [false, true] {
                assert_eq!(
                    glyph(true, icon, is_dark_panel, true),
                    Glyph::Planet {
                        colour: Colour::Black,
                        is_unread: true
                    }
                );
            }
        }
    }

    #[test]
    fn elsewhere_the_tray_is_the_app_icon_unless_monochrome() {
        assert_eq!(glyph(false, QUIET, true, true), Glyph::AppIcon);
        assert_eq!(
            glyph(false, MONOCHROME, true, false),
            Glyph::Planet {
                colour: Colour::White,
                is_unread: false
            }
        );
        assert_eq!(
            glyph(false, MONOCHROME, false, true),
            Glyph::Planet {
                colour: Colour::Black,
                is_unread: true
            }
        );
    }

    #[test]
    fn a_gnome_top_bar_is_dark_whatever_the_theme() {
        assert!(is_dark_panel_on("ubuntu:GNOME", Some(Theme::Light)));
        assert!(is_dark_panel_on("KDE", Some(Theme::Dark)));
        assert!(!is_dark_panel_on("KDE", Some(Theme::Light)));
        assert!(!is_dark_panel_on("", None));
    }

    #[test]
    fn every_glyph_has_the_size_it_claims() {
        for colour in [Colour::Black, Colour::White] {
            for is_unread in [false, true] {
                assert_eq!(
                    planet_pixels(colour, is_unread).len(),
                    (GLYPH_PIXELS * GLYPH_PIXELS * 4) as usize
                );
            }
        }
    }
}
