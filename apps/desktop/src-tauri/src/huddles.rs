//! One command per huddle call on the workspace.

use shy_core::{
    huddle::media::DeviceChoice,
    views::{Devices, Huddle, MyHuddle},
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

#[tauri::command]
pub async fn join_huddle(state: State<'_, AppState>, channel: String, muted: bool) -> CommandResult<()> {
    current(&state)
        .await?
        .join_huddle(&channel, muted)
        .map_err(failed)
}

#[tauri::command]
pub async fn leave_huddle(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.leave_huddle().map_err(failed)
}

#[tauri::command]
pub async fn set_huddle_muted(state: State<'_, AppState>, muted: bool) -> CommandResult<()> {
    current(&state).await?.set_huddle_muted(muted).map_err(failed)
}

#[tauri::command]
pub async fn huddle_devices(state: State<'_, AppState>) -> CommandResult<Devices> {
    current(&state).await?.huddle_devices().map_err(failed)
}

#[tauri::command]
pub async fn set_huddle_devices(state: State<'_, AppState>, choice: DeviceChoice) -> CommandResult<()> {
    current(&state).await?.set_huddle_devices(choice).map_err(failed)
}
