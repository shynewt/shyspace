//! One command per huddle call. The one call this device can be in, and a
//! ring, act on the workspace they belong to, whichever is shown.

use shy_core::{
    huddle::media::DeviceChoice,
    views::{Devices, Huddle, WorkspaceHuddle, WorkspaceRing},
};
use tauri::State;

use crate::{AppState, CommandResult, current, failed};

#[tauri::command]
pub async fn huddle(state: State<'_, AppState>, channel: String) -> CommandResult<Huddle> {
    current(&state).await?.huddle(&channel).map_err(failed)
}

/// This device's call, wherever it is, with its workspace and place name.
#[tauri::command]
pub async fn my_huddle(state: State<'_, AppState>) -> CommandResult<Option<WorkspaceHuddle>> {
    state.workspaces.my_huddle().map_err(failed)
}

/// Joins a huddle in the selected workspace, leaving any call in the
/// others first: audio is shared, so one call at a time.
#[tauri::command]
pub async fn join_huddle(state: State<'_, AppState>, channel: String, muted: bool) -> CommandResult<()> {
    let workspace = current(&state).await?;
    state
        .workspaces
        .join_huddle(workspace.workspace_id(), &channel, muted)
        .map_err(failed)
}

#[tauri::command]
pub async fn leave_huddle(state: State<'_, AppState>) -> CommandResult<()> {
    match state.workspaces.in_huddle() {
        Some(workspace) => workspace.leave_huddle().map_err(failed),
        None => Ok(()),
    }
}

/// A call ringing in any workspace, with that workspace's id.
#[tauri::command]
pub async fn huddle_ringing(state: State<'_, AppState>) -> CommandResult<Option<WorkspaceRing>> {
    Ok(state.workspaces.ringing())
}

/// Stops the ring in `workspace_id`'s workspace.
#[tauri::command]
pub async fn decline_huddle(state: State<'_, AppState>, workspace_id: String) -> CommandResult<()> {
    if let Some(workspace) = state.workspaces.find(&workspace_id) {
        workspace.decline_huddle();
    }
    Ok(())
}

#[tauri::command]
pub async fn set_huddle_muted(state: State<'_, AppState>, muted: bool) -> CommandResult<()> {
    let workspace = state
        .workspaces
        .in_huddle()
        .ok_or_else(|| "not in a huddle".to_owned())?;
    workspace.set_huddle_muted(muted).map_err(failed)
}

#[tauri::command]
pub async fn huddle_devices(state: State<'_, AppState>) -> CommandResult<Devices> {
    let workspace = current(&state).await?;
    // Asking the system for its devices can take a moment.
    tauri::async_runtime::spawn_blocking(move || workspace.huddle_devices())
        .await
        .map_err(|error| format!("listing the audio devices crashed: {error}"))?
        .map_err(failed)
}

#[tauri::command]
pub async fn set_huddle_devices(state: State<'_, AppState>, choice: DeviceChoice) -> CommandResult<()> {
    current(&state).await?.set_huddle_devices(choice).map_err(failed)
}

#[tauri::command]
pub async fn huddle_sounds(state: State<'_, AppState>) -> CommandResult<bool> {
    current(&state).await?.huddle_sounds().map_err(failed)
}

#[tauri::command]
pub async fn set_huddle_sounds(state: State<'_, AppState>, is_on: bool) -> CommandResult<()> {
    current(&state).await?.set_huddle_sounds(is_on).map_err(failed)
}
