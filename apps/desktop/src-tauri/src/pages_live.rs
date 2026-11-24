//! Who else is on a page and where their cursors are.

use shy_core::{pages::cursors::CursorSelection, views::PagePresence};
use tauri::State;

use crate::{AppState, CommandResult, current, failed};

#[tauri::command]
pub async fn join_page(state: State<'_, AppState>, page: String, share: bool) -> CommandResult<()> {
    current(&state)
        .await?
        .join_page(&page, share)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn leave_page(state: State<'_, AppState>, page: String) -> CommandResult<()> {
    current(&state).await?.leave_page(&page).await.map_err(failed)
}

#[tauri::command]
pub async fn move_cursor(
    state: State<'_, AppState>,
    page: String,
    selection: Option<CursorSelection>,
) -> CommandResult<()> {
    current(&state)
        .await?
        .move_cursor(&page, selection)
        .await
        .map_err(failed)
}

#[tauri::command]
pub async fn page_presence(state: State<'_, AppState>, page: String) -> CommandResult<PagePresence> {
    current(&state).await?.page_presence(&page).map_err(failed)
}
