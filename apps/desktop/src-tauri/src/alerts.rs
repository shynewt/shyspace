//! Desktop alerts. A click on one brings the window forward and tells the
//! frontend to open what it was about: a message, or a channel (a huddle).
//! On macOS, in the installed app, they go through Apple's notification
//! center (`notify_macos`), which also offers quick actions
//! (`alert_actions`); elsewhere through Tauri's notification plugin.

use std::{
    collections::VecDeque,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicI32, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::alert_actions::{AlertAction, PendingActions};
use crate::tray;

/// Tells the frontend a click or button on an alert waits for it (`take_alert_actions`).
const ACTION_EVENT: &str = "alert-action";
/// Alerts older than this many can't be followed to their target any more.
const REMEMBERED: usize = 100;

/// What a click on an alert opens, as `{ "kind": "message" | "channel", "id": … }`,
/// or `{ "kind": "page", "id": …, "position": [bytes] }` for a spot in a page.
/// The frontend checks it when it opens it; here it's only carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlertTarget {
    Message { id: String },
    Channel { id: String },
    Page { id: String, position: Vec<u8> },
}

/// What an alert is about, and in which workspace. On macOS it rides in the
/// notification's identifier as `{ "workspace_id": …, "kind": …, "id": … }`,
/// so a response finds it after a restart; changing that form strands the
/// notifications already shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenedAlert {
    pub workspace_id: Option<String>,
    #[serde(flatten)]
    pub target: AlertTarget,
}

/// What each recent alert was about, by the alert's id.
#[derive(Default)]
pub struct AlertTargets {
    next_id: AtomicI32,
    recent: Mutex<VecDeque<(i32, OpenedAlert)>>,
}

impl AlertTargets {
    fn next_id(&self) -> i32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn remember(&self, target: OpenedAlert) -> i32 {
        let id = self.next_id();
        // The list only ever holds whole entries, so a panic elsewhere can't leave it broken.
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if recent.len() == REMEMBERED {
            recent.pop_front();
        }
        recent.push_back((id, target));
        id
    }

    fn target_of(&self, id: i32) -> Option<OpenedAlert> {
        let recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        recent
            .iter()
            .find(|(alert, _)| *alert == id)
            .map(|(_, target)| target.clone())
    }
}

/// Listens for clicks on alerts, and on macOS for their quick actions, for
/// the life of the app. On the main thread, before the event loop runs, so
/// a response that launched the app isn't missed.
pub fn follow_clicks(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AlertTargets::default());
    app.manage(PendingActions::default());
    #[cfg(target_os = "macos")]
    if crate::notify_macos::is_available() {
        let handle = app.clone();
        crate::notify_macos::start(move |identifier, action, typed| {
            if let Some(response) = crate::alert_actions::response_to(&identifier, &action, typed) {
                respond(&handle, response);
            }
        });
        return Ok(());
    }
    let handle = app.clone();
    app.notification()
        .on_action(move |performed| {
            if performed.action_id() != "tap" {
                return;
            }
            let Some(id) = performed.notification().map(|notification| notification.id()) else {
                return;
            };
            if let Some(alert) = handle.state::<AlertTargets>().target_of(id) {
                respond(&handle, AlertAction::opening(alert));
            }
        })
        .map_err(|error| tauri::Error::Anyhow(error.into()))
}

fn respond(app: &AppHandle, action: AlertAction) {
    if matches!(action, AlertAction::Open { .. }) {
        tray::show_main_window(app);
    }
    app.state::<PendingActions>().push(action);
    if let Err(error) = app.emit(ACTION_EVENT, ()) {
        tracing::warn!("could not pass on what was done with an alert: {error}");
    }
}

/// The picture shown with an alert: PNG bytes of a member's avatar.
#[derive(Deserialize)]
pub struct AlertPicture {
    member: String,
    png: Vec<u8>,
}

/// Shows an alert; a click on it opens `target` (in `workspace_id`'s
/// workspace) if there is one.
pub fn show(
    app: &AppHandle,
    workspace_id: Option<String>,
    title: String,
    body: String,
    target: Option<AlertTarget>,
    picture: Option<AlertPicture>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if crate::notify_macos::is_available() {
        let id = app.state::<AlertTargets>().next_id();
        let alert = target.map(|target| OpenedAlert { workspace_id, target });
        let identifier = match &alert {
            Some(alert) => crate::alert_actions::alert_identifier(id, alert)?,
            None => id.to_string(),
        };
        let category = alert.as_ref().and_then(crate::alert_actions::AlertCategory::of);
        let picture = picture.map(|picture| save_picture(app, &picture)).transpose()?;
        return crate::notify_macos::show(&identifier, &title, &body, category, picture.as_deref());
    }
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(target) = target {
        let opened = OpenedAlert { workspace_id, target };
        builder = builder.id(app.state::<AlertTargets>().remember(opened));
    }
    if let Some(picture) = picture {
        builder = builder.icon(save_picture(app, &picture)?);
    }
    builder.show().map_err(|error| error.to_string())
}

/// Most bytes a picture may take; an avatar needs a few kilobytes.
const MAX_PICTURE_BYTES: usize = 256 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Writes `picture` where the notification service can read it: one file per
/// member in the app's cache folder. Returns its path.
fn save_picture(app: &AppHandle, picture: &AlertPicture) -> Result<String, String> {
    let file_name = picture_file_name(picture)?;
    let folder = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("finding the cache folder: {error}"))?
        .join("alert-pictures");
    std::fs::create_dir_all(&folder).map_err(|error| format!("making {}: {error}", folder.display()))?;
    let path = folder.join(file_name);
    std::fs::write(&path, &picture.png).map_err(|error| format!("writing {}: {error}", path.display()))?;
    path.into_os_string()
        .into_string()
        .map_err(|_| "the cache folder's path isn't valid text".into())
}

/// The file `picture` is saved as, once it checks out: a small PNG named for
/// a member id, so nothing from the page can pick another path.
fn picture_file_name(picture: &AlertPicture) -> Result<String, String> {
    let is_member_id =
        picture.member.len() == 64 && picture.member.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !is_member_id {
        return Err("an alert picture needs a member id".into());
    }
    if picture.png.len() > MAX_PICTURE_BYTES || !picture.png.starts_with(PNG_SIGNATURE) {
        return Err("an alert picture must be a small PNG".into());
    }
    Ok(format!("{}.png", picture.member.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(member: &str, png: &[u8]) -> AlertPicture {
        AlertPicture {
            member: member.into(),
            png: png.to_vec(),
        }
    }

    #[test]
    fn a_picture_is_named_for_its_member() {
        let member = "AB".repeat(32);
        let png = [PNG_SIGNATURE, b"rest"].concat();
        assert_eq!(
            picture_file_name(&picture(&member, &png)),
            Ok(format!("{}.png", "ab".repeat(32)))
        );
    }

    #[test]
    fn a_picture_can_only_name_a_member() {
        let png = [PNG_SIGNATURE, b"rest"].concat();
        for member in [
            "../../.bashrc",
            "",
            &"a".repeat(63),
            &format!("{}/x", "a".repeat(62)),
        ] {
            assert!(picture_file_name(&picture(member, &png)).is_err(), "{member}");
        }
    }

    #[test]
    fn a_picture_must_be_a_small_png() {
        let member = "a".repeat(64);
        assert!(picture_file_name(&picture(&member, b"GIF89a")).is_err());
        let huge = [PNG_SIGNATURE, &vec![0; MAX_PICTURE_BYTES]].concat();
        assert!(picture_file_name(&picture(&member, &huge)).is_err());
    }
}
