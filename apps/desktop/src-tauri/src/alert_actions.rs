//! What someone does with an alert: a click opens what it's about, and on
//! macOS "Reply" sends typed text where the alerted message is and "Mark as
//! read" reads its conversation. Each waits here until the frontend takes
//! it: one that launched the app comes before the frontend listens, a reply
//! needs the composer's mention handling, and its workspace may still be
//! opening.

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use shy_core::{Workspace, views::Member};
use tauri::{AppHandle, State};

use crate::alerts::{self, Alert, AlertKind, AlertTarget, OpenedAlert};
use crate::{AppState, CommandResult, failed};

/// The action ids an alert's buttons report.
#[cfg(any(target_os = "macos", test))]
pub const OPEN_ACTION: &str = "open";
#[cfg(any(target_os = "macos", test))]
pub const REPLY_ACTION: &str = "reply";
#[cfg(any(target_os = "macos", test))]
pub const MARK_READ_ACTION: &str = "mark-read";

/// Which buttons an alert offers, by what it's about.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertCategory {
    /// Reply and Mark as read.
    Message,
    /// Mark as read only: a page isn't answered from a notification.
    Page,
}

#[cfg(any(target_os = "macos", test))]
impl AlertCategory {
    #[cfg(target_os = "macos")]
    pub const ALL: [AlertCategory; 2] = [AlertCategory::Message, AlertCategory::Page];

    #[cfg(target_os = "macos")]
    pub fn id(self) -> &'static str {
        match self {
            AlertCategory::Message => "message",
            AlertCategory::Page => "page",
        }
    }

    /// The buttons an alert of `kind` shows, if any.
    pub fn shown_for(alert: &OpenedAlert, kind: AlertKind) -> Option<AlertCategory> {
        match kind {
            AlertKind::Message | AlertKind::PageMention => AlertCategory::of(alert),
            AlertKind::Huddle | AlertKind::Reminder | AlertKind::Notice => None,
        }
    }

    /// None for an alert with no buttons: a huddle, or one outside any workspace.
    pub fn of(alert: &OpenedAlert) -> Option<AlertCategory> {
        alert.workspace_id.as_ref()?;
        match alert.target {
            AlertTarget::Message { .. } => Some(AlertCategory::Message),
            AlertTarget::Page { .. } => Some(AlertCategory::Page),
            AlertTarget::Channel { .. } => None,
        }
    }
}

/// What someone did with an alert, as the frontend takes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AlertAction {
    /// A click: open what it's about (in the shown workspace if it names none).
    Open {
        workspace_id: Option<String>,
        target: AlertTarget,
    },
    /// Only macOS offers buttons, for now (#270).
    #[cfg(any(target_os = "macos", test))]
    Reply {
        workspace_id: String,
        message: String,
        text: String,
    },
    #[cfg(any(target_os = "macos", test))]
    MarkRead {
        workspace_id: String,
        target: AlertTarget,
    },
    /// A reply that waited too long to be sent: kept as a draft instead,
    /// never sent when its moment has passed.
    #[cfg(any(target_os = "macos", test))]
    KeepAsDraft {
        workspace_id: String,
        message: String,
        text: String,
    },
}

impl AlertAction {
    pub fn opening(alert: OpenedAlert) -> AlertAction {
        AlertAction::Open {
            workspace_id: alert.workspace_id,
            target: alert.target,
        }
    }

    fn workspace_id(&self) -> Option<&str> {
        match self {
            AlertAction::Open { workspace_id, .. } => workspace_id.as_deref(),
            #[cfg(any(target_os = "macos", test))]
            AlertAction::Reply { workspace_id, .. }
            | AlertAction::MarkRead { workspace_id, .. }
            | AlertAction::KeepAsDraft { workspace_id, .. } => Some(workspace_id),
        }
    }
}

/// The notification id for an alert: unique in this run, and carrying what
/// the alert is about, so a response still finds it after a restart.
#[cfg(any(target_os = "macos", test))]
pub fn alert_identifier(sequence: i32, alert: &OpenedAlert) -> Result<String, String> {
    let about = serde_json::to_string(alert).map_err(|error| error.to_string())?;
    Ok(format!("{sequence}.{about}"))
}

#[cfg(any(target_os = "macos", test))]
fn alert_of(identifier: &str) -> Option<OpenedAlert> {
    let (_, about) = identifier.split_once('.')?;
    serde_json::from_str(about).ok()
}

/// What a response to the notification `identifier` asks for; None for one
/// that asks nothing (a dismissal, a blank reply) or isn't one of ours.
#[cfg(any(target_os = "macos", test))]
pub fn response_to(identifier: &str, action: &str, typed: Option<String>) -> Option<AlertAction> {
    let alert = alert_of(identifier)?;
    if action == OPEN_ACTION {
        return Some(AlertAction::opening(alert));
    }
    let category = AlertCategory::of(&alert)?;
    let workspace_id = alert.workspace_id?;
    match (action, category, alert.target) {
        (REPLY_ACTION, AlertCategory::Message, AlertTarget::Message { id }) => {
            let text = typed.filter(|text| !text.trim().is_empty())?;
            Some(AlertAction::Reply {
                workspace_id,
                message: id,
                text,
            })
        }
        (MARK_READ_ACTION, _, target) => Some(AlertAction::MarkRead { workspace_id, target }),
        _ => None,
    }
}

/// How long an action waits for its workspace to open: a reply typed long
/// ago mustn't go out much later, when its moment has passed.
const ACTIONS_EXPIRE_AFTER: Duration = Duration::from_secs(5 * 60);

/// Where an action's workspace is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePlace {
    Open,
    /// On this device, not open yet.
    Closed,
    /// Not on this device (left, or never here).
    Gone,
}

/// Alert actions not yet taken by the frontend, with when each came.
#[derive(Default)]
pub struct PendingActions(Mutex<Vec<(Instant, AlertAction)>>);

impl PendingActions {
    pub fn push(&self, action: AlertAction) {
        self.push_at(action, Instant::now());
    }

    fn push_at(&self, action: AlertAction, now: Instant) {
        // The list only ever holds whole entries, so a panic elsewhere can't leave it broken.
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((now, action));
    }

    /// Takes the actions whose workspace is open, or that name none. A
    /// reply that waited too long becomes a draft to keep; other actions
    /// that did are dropped, as are those whose workspace isn't on this
    /// device. The rest wait for theirs.
    pub fn take_where(&self, now: Instant, place: impl Fn(&str) -> WorkspacePlace) -> Vec<AlertAction> {
        let mut pending = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut ready = Vec::new();
        let mut waiting = Vec::new();
        for (came, action) in pending.drain(..) {
            let is_stale = now.duration_since(came) > ACTIONS_EXPIRE_AFTER;
            let action = match (action, is_stale) {
                #[cfg(any(target_os = "macos", test))]
                (
                    AlertAction::Reply {
                        workspace_id,
                        message,
                        text,
                    },
                    true,
                ) => AlertAction::KeepAsDraft {
                    workspace_id,
                    message,
                    text,
                },
                #[cfg(any(target_os = "macos", test))]
                (keep @ AlertAction::KeepAsDraft { .. }, _) => keep,
                (_, true) => {
                    tracing::info!("dropped an alert action that waited too long");
                    continue;
                }
                (action, false) => action,
            };
            match action.workspace_id().map_or(WorkspacePlace::Open, &place) {
                WorkspacePlace::Open => ready.push(action),
                WorkspacePlace::Closed => waiting.push((came, action)),
                WorkspacePlace::Gone => tracing::info!("dropped an alert action: its workspace isn't here"),
            }
        }
        *pending = waiting;
        ready
    }
}

/// Whether macOS lets shyspace show notifications. Elsewhere, and in
/// development builds on macOS, it's always `Allowed`: nothing to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    NotAsked,
    Allowed,
    Denied,
}

fn open_workspace(state: &AppState, workspace_id: &str) -> CommandResult<Workspace> {
    state
        .workspaces
        .find(workspace_id)
        .ok_or_else(|| "that workspace isn't open".to_owned())
}

/// The alert actions waiting whose workspace is open.
#[tauri::command]
pub fn take_alert_actions(
    state: State<'_, AppState>,
    pending: State<'_, PendingActions>,
) -> Vec<AlertAction> {
    let here = match state.workspaces.list() {
        Ok(entries) => Some(
            entries
                .into_iter()
                .map(|entry| entry.workspace_id)
                .collect::<Vec<_>>(),
        ),
        Err(error) => {
            tracing::warn!("couldn't list this device's workspaces: {error:#}");
            None
        }
    };
    pending.take_where(Instant::now(), |workspace_id| {
        if state.workspaces.find(workspace_id).is_some() {
            return WorkspacePlace::Open;
        }
        // Unknown when the list can't be read: kept until it expires.
        match &here {
            Some(here) if !here.iter().any(|id| id == workspace_id) => WorkspacePlace::Gone,
            _ => WorkspacePlace::Closed,
        }
    })
}

/// Who a reply typed in a notification can mention, in any open workspace.
#[derive(Serialize)]
pub struct PeopleIn {
    me: String,
    members: Vec<Member>,
}

#[tauri::command]
pub fn people_in(state: State<'_, AppState>, workspace_id: String) -> CommandResult<PeopleIn> {
    let workspace = open_workspace(&state, &workspace_id)?;
    Ok(PeopleIn {
        me: workspace.my_id(),
        members: workspace.members().map_err(failed)?,
    })
}

/// Sends a reply typed in a notification, with its mentions already written.
#[tauri::command]
pub fn reply_to_alert(
    state: State<'_, AppState>,
    workspace_id: String,
    message: String,
    text: String,
) -> CommandResult<()> {
    open_workspace(&state, &workspace_id)?
        .reply_to_alert(&message, &text)
        .map_err(failed)
}

/// Keeps a reply that wasn't sent as a draft where it was going, and says
/// so in another notification, so it never just vanishes.
#[tauri::command]
pub fn keep_unsent_reply(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    message: String,
    text: String,
) -> CommandResult<()> {
    open_workspace(&state, &workspace_id)?
        .keep_unsent_reply(&message, &text)
        .map_err(failed)?;
    alerts::show(
        &app,
        Alert {
            kind: AlertKind::Notice,
            workspace_id: Some(workspace_id),
            title: "Your reply wasn't sent".into(),
            body: "It's saved as a draft.".into(),
            target: Some(AlertTarget::Message { id: message }),
            picture: None,
        },
    )
}

#[tauri::command]
pub fn mark_alert_read(
    state: State<'_, AppState>,
    workspace_id: String,
    target: AlertTarget,
) -> CommandResult<()> {
    let workspace = open_workspace(&state, &workspace_id)?;
    match target {
        AlertTarget::Message { id } => workspace.mark_alert_read(&id),
        AlertTarget::Page { id, .. } => workspace.mark_page_mentions_read(&id),
        AlertTarget::Channel { .. } => return Err("a huddle has nothing to mark read".into()),
    }
    .map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(workspace_id: Option<&str>, target: AlertTarget) -> OpenedAlert {
        OpenedAlert {
            workspace_id: workspace_id.map(str::to_owned),
            target,
        }
    }

    fn message(id: &str) -> AlertTarget {
        AlertTarget::Message { id: id.into() }
    }

    fn identifier(alert: &OpenedAlert) -> String {
        alert_identifier(7, alert).unwrap()
    }

    #[test]
    fn a_click_opens_what_the_alert_was_about() {
        let about = alert(Some("w1"), message("m1"));
        assert_eq!(
            response_to(&identifier(&about), OPEN_ACTION, None),
            Some(AlertAction::opening(about))
        );
    }

    #[test]
    fn a_reply_carries_its_text_to_the_alerted_message() {
        let about = alert(Some("w1"), message("m1"));
        assert_eq!(
            response_to(&identifier(&about), REPLY_ACTION, Some("on my way".into())),
            Some(AlertAction::Reply {
                workspace_id: "w1".into(),
                message: "m1".into(),
                text: "on my way".into(),
            })
        );
    }

    #[test]
    fn a_blank_reply_asks_nothing() {
        let about = alert(Some("w1"), message("m1"));
        assert_eq!(
            response_to(&identifier(&about), REPLY_ACTION, Some("  ".into())),
            None
        );
        assert_eq!(response_to(&identifier(&about), REPLY_ACTION, None), None);
    }

    #[test]
    fn mark_as_read_works_for_messages_and_pages() {
        for target in [
            message("m1"),
            AlertTarget::Page {
                id: "p1".into(),
                position: vec![1, 2],
            },
        ] {
            let about = alert(Some("w1"), target.clone());
            assert_eq!(
                response_to(&identifier(&about), MARK_READ_ACTION, None),
                Some(AlertAction::MarkRead {
                    workspace_id: "w1".into(),
                    target,
                })
            );
        }
    }

    #[test]
    fn a_page_gets_no_reply() {
        let about = alert(
            Some("w1"),
            AlertTarget::Page {
                id: "p1".into(),
                position: vec![],
            },
        );
        assert_eq!(AlertCategory::of(&about), Some(AlertCategory::Page));
        assert_eq!(
            response_to(&identifier(&about), REPLY_ACTION, Some("hi".into())),
            None
        );
    }

    #[test]
    fn only_message_and_page_mention_alerts_get_buttons() {
        let about_a_message = alert(Some("w1"), message("m1"));
        assert_eq!(
            AlertCategory::shown_for(&about_a_message, AlertKind::Message),
            Some(AlertCategory::Message)
        );
        for kind in [AlertKind::Reminder, AlertKind::Notice, AlertKind::Huddle] {
            assert_eq!(AlertCategory::shown_for(&about_a_message, kind), None, "{kind:?}");
        }
    }

    #[test]
    fn huddles_and_alerts_outside_a_workspace_get_no_buttons() {
        let huddle = alert(Some("w1"), AlertTarget::Channel { id: "c1".into() });
        let outside = alert(None, message("m1"));
        assert_eq!(AlertCategory::of(&huddle), None);
        assert_eq!(AlertCategory::of(&outside), None);
        assert_eq!(response_to(&identifier(&huddle), MARK_READ_ACTION, None), None);
        assert_eq!(
            response_to(&identifier(&outside), REPLY_ACTION, Some("hi".into())),
            None
        );
    }

    #[test]
    fn a_notification_that_isnt_ours_is_ignored() {
        for identifier in ["", "7", "7.not json", "7.{\"kind\":\"elsewhere\"}"] {
            assert_eq!(response_to(identifier, OPEN_ACTION, None), None, "{identifier}");
        }
    }

    #[test]
    fn actions_wait_until_their_workspace_is_open() {
        let pending = PendingActions::default();
        let read = |workspace_id: &str| AlertAction::MarkRead {
            workspace_id: workspace_id.into(),
            target: message("m1"),
        };
        let click = AlertAction::Open {
            workspace_id: None,
            target: message("m1"),
        };
        let now = Instant::now();
        pending.push_at(read("open"), now);
        pending.push_at(read("opening"), now);
        pending.push_at(click.clone(), now);
        let opening_later = |id: &str| match id {
            "open" => WorkspacePlace::Open,
            _ => WorkspacePlace::Closed,
        };

        assert_eq!(pending.take_where(now, opening_later), vec![read("open"), click]);
        assert_eq!(pending.take_where(now, opening_later), vec![]);
        assert_eq!(
            pending.take_where(now, |_| WorkspacePlace::Open),
            vec![read("opening")]
        );
    }

    #[test]
    fn a_reply_typed_long_ago_is_kept_as_a_draft_not_sent() {
        let pending = PendingActions::default();
        let reply = AlertAction::Reply {
            workspace_id: "w".into(),
            message: "m1".into(),
            text: "on my way".into(),
        };
        let typed = Instant::now();
        pending.push_at(reply.clone(), typed);
        let later = typed + ACTIONS_EXPIRE_AFTER + Duration::from_secs(1);

        assert_eq!(pending.take_where(later, |_| WorkspacePlace::Closed), vec![]);
        assert_eq!(
            pending.take_where(later, |_| WorkspacePlace::Open),
            vec![AlertAction::KeepAsDraft {
                workspace_id: "w".into(),
                message: "m1".into(),
                text: "on my way".into(),
            }],
            "waits for its workspace, then is kept"
        );

        pending.push_at(reply.clone(), typed);
        let soon = typed + Duration::from_secs(60);
        assert_eq!(pending.take_where(soon, |_| WorkspacePlace::Open), vec![reply]);
    }

    #[test]
    fn actions_for_a_workspace_no_longer_here_are_dropped() {
        let pending = PendingActions::default();
        let now = Instant::now();
        pending.push_at(
            AlertAction::MarkRead {
                workspace_id: "left".into(),
                target: message("m1"),
            },
            now,
        );

        assert_eq!(pending.take_where(now, |_| WorkspacePlace::Gone), vec![]);
        assert_eq!(
            pending.take_where(now, |_| WorkspacePlace::Open),
            vec![],
            "dropped, not kept"
        );
    }
}
