//! Desktop alerts. A click on one about a message brings the window forward
//! and tells the frontend to open that message.

use std::{
    collections::VecDeque,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicI32, Ordering},
    },
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::tray;

/// Sent to the frontend with the id of the message a clicked alert was about.
const OPENED_EVENT: &str = "alert-opened";
/// Alerts older than this many can't be followed to their message any more.
const REMEMBERED: usize = 100;

/// Which message each recent alert was about, by the alert's id.
#[derive(Default)]
pub struct AlertTargets {
    next_id: AtomicI32,
    recent: Mutex<VecDeque<(i32, String)>>,
}

impl AlertTargets {
    fn remember(&self, message: String) -> i32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // The list only ever holds whole entries, so a panic elsewhere can't leave it broken.
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if recent.len() == REMEMBERED {
            recent.pop_front();
        }
        recent.push_back((id, message));
        id
    }

    fn message_of(&self, id: i32) -> Option<String> {
        let recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        recent
            .iter()
            .find(|(alert, _)| *alert == id)
            .map(|(_, message)| message.clone())
    }
}

/// Listens for clicks on alerts for the life of the app.
pub fn follow_clicks(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AlertTargets::default());
    let handle = app.clone();
    app.notification()
        .on_action(move |performed| {
            if performed.action_id() != "tap" {
                return;
            }
            let Some(id) = performed.notification().map(|notification| notification.id()) else {
                return;
            };
            let Some(message) = handle.state::<AlertTargets>().message_of(id) else {
                return;
            };
            tray::show_main_window(&handle);
            if let Err(error) = handle.emit(OPENED_EVENT, message) {
                tracing::warn!("could not open the alert's message: {error}");
            }
        })
        .map_err(|error| tauri::Error::Anyhow(error.into()))
}

/// Shows an alert; a click on it opens `message` if there is one.
pub fn show(app: &AppHandle, title: String, body: String, message: Option<String>) -> Result<(), String> {
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(message) = message {
        builder = builder.id(app.state::<AlertTargets>().remember(message));
    }
    builder.show().map_err(|error| error.to_string())
}
