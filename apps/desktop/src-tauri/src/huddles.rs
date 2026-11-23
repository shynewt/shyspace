//! One command per huddle call on the workspace.

use shy_core::{
    huddle::media::DeviceChoice,
    views::{Devices, Huddle, IncomingHuddle, MyHuddle},
};
use tauri::State;

use crate::{AppState, CommandResult, current, failed};

#[tauri::command]
pub async fn huddle(state: State<'_, AppState>, channel: String) -> CommandResult<Huddle> {
    current(&state).await?.huddle(&channel).map_err(failed)
}

#[tauri::command]
pub async fn my_huddle(state: State<'_, AppState>) -> CommandResult<Option<MyHuddle>> {
    Ok(current(&state).await?.my_huddle())
}

/// Joins a huddle in the selected workspace, leaving any huddle in the
/// others first: audio is shared, so one huddle at a time.
#[tauri::command]
pub async fn join_huddle(state: State<'_, AppState>, channel: String, muted: bool) -> CommandResult<()> {
    let workspace = current(&state).await?;
    for other in state.open_workspaces() {
        if other.workspace_id() != workspace.workspace_id() && other.my_huddle().is_some() {
            other.leave_huddle().map_err(failed)?;
        }
    }
    workspace.join_huddle(&channel, muted).map_err(failed)
}

#[tauri::command]
pub async fn leave_huddle(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.leave_huddle().map_err(failed)
}

#[tauri::command]
pub async fn huddle_ringing(state: State<'_, AppState>) -> CommandResult<Option<IncomingHuddle>> {
    Ok(current(&state).await?.huddle_ringing())
}

#[tauri::command]
pub async fn decline_huddle(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.decline_huddle();
    Ok(())
}

#[tauri::command]
pub async fn set_huddle_muted(state: State<'_, AppState>, muted: bool) -> CommandResult<()> {
    current(&state).await?.set_huddle_muted(muted).map_err(failed)
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
