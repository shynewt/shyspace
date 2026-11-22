//! The main window, built here rather than from tauri.conf.json so it can
//! refuse to leave the app: it never navigates to another site, opens a new
//! window or starts a download. Links from members open in the system
//! browser through the opener instead, after the app's own checks.

use anyhow::Context;
use tauri::{AppHandle, Url, WebviewWindowBuilder, webview::NewWindowResponse};

use crate::tray::MAIN_WINDOW;

pub fn create_main_window(app: &AppHandle) -> anyhow::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW)
        .context("tauri.conf.json has no main window")?
        .clone();
    let dev_url = tauri::is_dev()
        .then(|| app.config().build.dev_url.clone())
        .flatten();
    WebviewWindowBuilder::from_config(app, &config)?
        .on_navigation(move |url| {
            let is_allowed = is_app_page(url, dev_url.as_ref());
            if !is_allowed {
                tracing::warn!("refused to leave the app for another page");
            }
            is_allowed
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .build()?;
    Ok(())
}

/// Whether `url` is one of the app's own pages: its bundled files (served
/// at `tauri://localhost`, or `http(s)://tauri.localhost` on Windows), or
/// the dev server while developing.
fn is_app_page(url: &Url, dev_url: Option<&Url>) -> bool {
    if dev_url.is_some_and(|dev_url| dev_url.origin() == url.origin()) {
        return true;
    }
    match (url.scheme(), url.host_str()) {
        ("tauri", Some("localhost")) => true,
        ("http" | "https", Some("tauri.localhost")) => url.port().is_none(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a valid test url")
    }

    #[test]
    fn the_apps_own_pages_are_allowed() {
        assert!(is_app_page(&url("tauri://localhost/index.html"), None));
        assert!(is_app_page(&url("http://tauri.localhost/#/channel/1"), None));
        assert!(is_app_page(&url("https://tauri.localhost/"), None));
    }

    #[test]
    fn other_sites_are_refused() {
        for other in [
            "https://evil.example/login",
            "https://tauri.localhost.evil.example/",
            "https://evil.example@tauri.localhost:8443/",
            "http://localhost:1430/",
            "file:///etc/passwd",
            "data:text/html,<p>hi</p>",
            "about:blank",
        ] {
            assert!(!is_app_page(&url(other), None), "{other} should be refused");
        }
    }

    #[test]
    fn the_dev_server_is_allowed_only_when_developing() {
        let dev_url = url("http://localhost:1430");
        assert!(is_app_page(
            &url("http://localhost:1430/settings"),
            Some(&dev_url)
        ));
        assert!(!is_app_page(&url("http://localhost:8080/"), Some(&dev_url)));
        assert!(!is_app_page(&url("http://localhost:1430/settings"), None));
    }
}
