//! Your devices: linking a new one from this device, linking this device to
//! a person who already uses shyspace, and the list of your devices. One
//! command per core call. The core runs each step of this device's link in
//! a task of its own; the wait for the admin's app goes on in the
//! background, also after a restart. What a screen must learn about this
//! device's link (one waiting after a restart, one that ended) is kept for
//! it to ask (`waiting_device_link`, `take_ended_device_link`): an event
//! (`link-ended`) only says to ask, so nothing is lost if no screen listens
//! yet.

use serde::Serialize;
use shy_core::{
    membership::link::Platform,
    views::{DeviceInfo, DeviceNotice, LinkCodeSummary},
    workspace::{CheckAfterRestart, CheckOnNewDevice, LinkCheckShown, LinkKeptUnfinished, Workspace},
    workspaces::{LinkPutAside, LinkStop, ResumedLinkStep, Workspaces},
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    AppState, CommandResult, Session, current, emit_workspaces_changed, failed, install, session_of,
};

/// What the new device shows once the person's other device opened its
/// request: the number to tap there, or nothing if they were quicker.
#[derive(Serialize, Clone)]
pub(crate) struct LinkCheck {
    code: Option<String>,
}

/// What this app knows about this device's link, for the screens.
#[derive(Default)]
pub(crate) struct LinkState {
    /// A link a restart interrupted before its hand-over.
    waiting: Option<CheckAfterRestart>,
    /// Why the last link ended without this device being added, until a
    /// screen takes it.
    ended: Option<String>,
    /// A reading link is shown and waited on already.
    is_following: bool,
}

#[tauri::command]
pub(crate) async fn create_link_code(state: State<'_, AppState>) -> CommandResult<String> {
    current(&state).await?.create_link_code().await.map_err(failed)
}

#[tauri::command]
pub(crate) async fn pending_link(state: State<'_, AppState>) -> CommandResult<Option<LinkCheckShown>> {
    current(&state).await?.pending_link().await.map_err(failed)
}

/// A new device confirmed: its id, and whether what's left finishes later
/// (the link is kept; the next sync pass sends it on).
#[derive(Serialize)]
pub(crate) struct ConfirmedLink {
    device: Option<String>,
    is_finishing: bool,
}

#[tauri::command]
pub(crate) async fn confirm_link(state: State<'_, AppState>, picked: String) -> CommandResult<ConfirmedLink> {
    match current(&state).await?.confirm_link(&picked).await {
        Ok(device) => Ok(ConfirmedLink {
            device: Some(device),
            is_finishing: false,
        }),
        Err(error) if error.downcast_ref::<LinkKeptUnfinished>().is_some() => {
            tracing::warn!("a confirmed link finishes later: {error:#}");
            Ok(ConfirmedLink {
                device: None,
                is_finishing: true,
            })
        }
        Err(error) => Err(failed(error)),
    }
}

#[tauri::command]
pub(crate) async fn cancel_link(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.cancel_link().await.map_err(failed)
}

#[tauri::command]
pub(crate) async fn my_devices(state: State<'_, AppState>) -> CommandResult<Vec<DeviceInfo>> {
    current(&state).await?.my_devices().map_err(failed)
}

#[tauri::command]
pub(crate) async fn remove_my_device(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    current(&state).await?.remove_my_device(&id).await.map_err(failed)
}

#[tauri::command]
pub(crate) async fn remove_my_device_and_its_links(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    current(&state)
        .await?
        .remove_my_device_and_its_links(&id)
        .await
        .map_err(failed)
}

#[tauri::command]
pub(crate) async fn device_notices(state: State<'_, AppState>) -> CommandResult<Vec<DeviceNotice>> {
    current(&state).await?.device_notices().map_err(failed)
}

/// The notices in one open workspace, shown or not, so each can alert.
#[tauri::command]
pub(crate) async fn workspace_device_notices(
    state: State<'_, AppState>,
    workspace_id: String,
) -> CommandResult<Vec<DeviceNotice>> {
    let workspace = state
        .workspaces
        .find(&workspace_id)
        .ok_or_else(|| "that workspace isn't open on this device".to_owned())?;
    workspace.device_notices().map_err(failed)
}

#[tauri::command]
pub(crate) async fn mark_device_notices_seen(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.mark_device_notices_seen().map_err(failed)
}

#[tauri::command]
pub(crate) fn read_link_code(code: String) -> CommandResult<LinkCodeSummary> {
    Workspaces::read_link_code(&code).map_err(failed)
}

/// Starts linking this device and waits for the person's other device to
/// open its request.
#[tauri::command]
pub(crate) async fn begin_device_link(state: State<'_, AppState>, code: String) -> CommandResult<LinkCheck> {
    let check = state
        .workspaces
        .begin_link(&code, this_platform())
        .await
        .map_err(failed)?;
    Ok(LinkCheck {
        code: match check {
            CheckOnNewDevice::Code(code) => Some(code),
            CheckOnNewDevice::SomeoneElse | CheckOnNewDevice::Confirmed => None,
        },
    })
}

/// Waits for what the other device hands over, then shows the workspace
/// for reading; the wait to be added goes on in the background. Asking
/// again shows the same workspace and starts nothing twice.
#[tauri::command]
pub(crate) async fn wait_for_device_link_handover(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Session> {
    let handed_over = state.workspaces.wait_for_link_handover().await;
    link_state(&state)?.waiting = None;
    let reading = handed_over.map_err(failed)?;
    follow_reading_link(&app, &reading)
}

/// Stops linking this device, for good: what it was handed is wiped. If
/// the admin's app added the device just before, the joined workspace
/// stays and is shown.
#[tauri::command]
pub(crate) async fn stop_device_link(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let stopped = state.workspaces.cancel_link().await.map_err(failed)?;
    match stopped {
        LinkStop::Stopped => {}
        LinkStop::AlreadyJoined(joined) => {
            install(&app, &joined);
        }
        LinkStop::PutAside => {
            return Err(
                "The app closed before the link stopped. Stop it again once the app is open.".to_owned(),
            );
        }
    }
    link_state(&state)?.waiting = None;
    emit_workspaces_changed(&app);
    Ok(())
}

/// Where a link a restart interrupted before its hand-over stood when last
/// asked; the screen asks again with `recheck_device_link`, then goes on
/// with `wait_for_device_link_handover`.
#[tauri::command]
pub(crate) fn waiting_device_link(state: State<'_, AppState>) -> CommandResult<Option<CheckAfterRestart>> {
    Ok(link_state(&state)?.waiting.clone())
}

/// Asks again where a link picked up after a restart stands.
#[tauri::command]
pub(crate) async fn recheck_device_link(state: State<'_, AppState>) -> CommandResult<CheckAfterRestart> {
    let rechecked = state.workspaces.recheck_link().await;
    // A link that ended is gone from disk; one busy for a moment still waits.
    let is_still_waiting = state.workspaces.has_link_in_progress().map_err(failed)?;
    let mut link = link_state(&state)?;
    match &rechecked {
        Ok(check) => link.waiting = Some(check.clone()),
        Err(_) if !is_still_waiting => link.waiting = None,
        Err(_) => {}
    }
    rechecked.map_err(failed)
}

/// Why this device's last link ended without it being added, once.
#[tauri::command]
pub(crate) fn take_ended_device_link(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    Ok(link_state(&state)?.ended.take())
}

/// Picks up a link a restart interrupted, after the workspaces opened. One
/// already reading opens and waits on in the background; one still waiting
/// for its hand-over is kept for the UI to ask about.
pub(crate) async fn resume_device_link(app: &AppHandle, state: &AppState) {
    match state.workspaces.resume_link().await {
        Ok(None) => {}
        Ok(Some(ResumedLinkStep::Reading(reading))) => {
            if let Err(error) = follow_reading_link(app, &reading) {
                tracing::error!("couldn't follow the link this device is reading through: {error}");
            }
        }
        Ok(Some(ResumedLinkStep::Waiting { check })) => match link_state(state) {
            Ok(mut link) => link.waiting = Some(check),
            Err(error) => tracing::error!("couldn't keep the interrupted link for the UI: {error}"),
        },
        Err(error) => tell_link_ended(app, &error),
    }
}

/// Shows the reading workspace and waits in the background until the
/// device is added, unless that runs already.
fn follow_reading_link(app: &AppHandle, reading: &Workspace) -> CommandResult<Session> {
    let state = app.state::<AppState>();
    let is_following = std::mem::replace(&mut link_state(&state)?.is_following, true);
    if is_following {
        return Ok(session_of(reading));
    }
    wait_until_linked_in_background(app);
    Ok(install(app, reading))
}

fn link_state(state: &AppState) -> CommandResult<std::sync::MutexGuard<'_, LinkState>> {
    state
        .link
        .lock()
        .map_err(|_| "link state lock poisoned".to_owned())
}

fn wait_until_linked_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let linked = state.workspaces.wait_until_linked(state.online()).await;
        match link_state(&state) {
            Ok(mut link) => link.is_following = false,
            Err(error) => tracing::error!("couldn't note the link's end: {error}"),
        }
        match linked {
            Ok(workspace) => {
                install(&app, &workspace);
            }
            Err(error) => {
                tell_link_ended(&app, &error);
                emit_workspaces_changed(&app);
            }
        }
    });
}

/// Keeps why the link ended for a screen to take, and says to ask. Closing
/// the app isn't an ending: the link picks up at the next start.
fn tell_link_ended(app: &AppHandle, error: &anyhow::Error) {
    if error.downcast_ref::<LinkPutAside>().is_some() {
        return;
    }
    tracing::warn!("linking this device ended: {error:#}");
    let state = app.state::<AppState>();
    match link_state(&state) {
        Ok(mut link) => link.ended = Some(format!("{error:#}")),
        Err(lock_error) => tracing::error!("couldn't keep why the link ended: {lock_error}"),
    }
    let _ = app.emit("link-ended", ());
}

fn this_platform() -> Platform {
    match std::env::consts::OS {
        "macos" => Platform::MacOs,
        "windows" => Platform::Windows,
        _ => Platform::Linux,
    }
}
