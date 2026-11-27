//! Your devices: linking a new one from this device, linking this device to
//! a person who already uses shyspace, and the list of your devices. One
//! command per core call. The core runs each step of this device's link in
//! a task of its own; the wait for the admin's app goes on in the
//! background, also after a restart, and tells the UI when it ends.

use serde::Serialize;
use shy_core::{
    membership::link::Platform,
    views::{DeviceInfo, DeviceNotice, LinkCodeSummary},
    workspace::{CheckOnNewDevice, LinkCheckShown},
    workspaces::{ResumedLinkStep, Workspaces},
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, CommandResult, Session, current, emit_workspaces_changed, failed, install};

/// What the new device shows once the person's other device opened its
/// request: the number to tap there, or nothing if they were quicker.
#[derive(Serialize, Clone)]
pub(crate) struct LinkCheck {
    code: Option<String>,
}

/// Why a link on this device ended without it being added, said once.
#[derive(Serialize, Clone)]
struct LinkEnded {
    message: String,
}

#[tauri::command]
pub(crate) async fn create_link_code(state: State<'_, AppState>) -> CommandResult<String> {
    current(&state).await?.create_link_code().await.map_err(failed)
}

#[tauri::command]
pub(crate) async fn pending_link(state: State<'_, AppState>) -> CommandResult<Option<LinkCheckShown>> {
    current(&state).await?.pending_link().await.map_err(failed)
}

#[tauri::command]
pub(crate) async fn confirm_link(state: State<'_, AppState>, picked: String) -> CommandResult<String> {
    current(&state).await?.confirm_link(&picked).await.map_err(failed)
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
/// for reading; the wait to be added goes on in the background.
#[tauri::command]
pub(crate) async fn wait_for_device_link_handover(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<Session> {
    let reading = state.workspaces.wait_for_link_handover().await.map_err(failed)?;
    wait_until_linked_in_background(&app);
    Ok(install(&app, &reading))
}

/// Stops linking this device, for good: what it was handed is wiped.
#[tauri::command]
pub(crate) async fn stop_device_link(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    state.workspaces.cancel_link().await.map_err(failed)?;
    emit_workspaces_changed(&app);
    Ok(())
}

/// Picks up a link a restart interrupted, after the workspaces opened. One
/// still waiting for its hand-over shows its number again
/// (`link-waiting`), and both wait on in the background.
pub(crate) async fn resume_device_link(app: &AppHandle, state: &AppState) {
    match state.workspaces.resume_link().await {
        Ok(None) => {}
        Ok(Some(ResumedLinkStep::Reading(reading))) => {
            install(app, &reading);
            wait_until_linked_in_background(app);
        }
        Ok(Some(ResumedLinkStep::Waiting { code })) => {
            let _ = app.emit("link-waiting", LinkCheck { code });
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                match state.workspaces.wait_for_link_handover().await {
                    Ok(reading) => {
                        install(&app, &reading);
                        wait_until_linked_in_background(&app);
                    }
                    Err(error) => tell_link_ended(&app, &error),
                }
            });
        }
        Err(error) => tell_link_ended(app, &error),
    }
}

fn wait_until_linked_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match state.workspaces.wait_until_linked(state.online()).await {
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

fn tell_link_ended(app: &AppHandle, error: &anyhow::Error) {
    tracing::warn!("linking this device ended: {error:#}");
    let _ = app.emit(
        "link-ended",
        LinkEnded {
            message: format!("{error:#}"),
        },
    );
}

fn this_platform() -> Platform {
    match std::env::consts::OS {
        "macos" => Platform::MacOs,
        "windows" => Platform::Windows,
        _ => Platform::Linux,
    }
}
