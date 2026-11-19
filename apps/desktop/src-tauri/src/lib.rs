//! Thin bridge from the UI to shy-core: every command maps to one Workspace
//! call, and every core change is forwarded to the UI as a "change" event.

use std::path::PathBuf;

use serde::Serialize;
use shy_core::{
    BucketConfig, DataDir, KeyStorage, Networking, Profile, Workspace,
    sync::SyncStatus,
    views::{Channel, Invite, Member, Message, MessageCursor, Page, SearchHit, StorageSettings},
    workspace::StorageUpdate,
};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Response};
use tokio::sync::Mutex;

type CommandResult<T> = Result<T, String>;

struct AppState {
    data: DataDir,
    workspace: Mutex<Option<Workspace>>,
}

#[derive(Serialize)]
struct Session {
    workspace_name: String,
    me: String,
    is_admin: bool,
}

fn failed(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn session_of(workspace: &Workspace) -> Session {
    Session {
        workspace_name: workspace.name().to_owned(),
        me: workspace.my_id(),
        is_admin: workspace.is_admin(),
    }
}

fn forward_changes(app: &AppHandle, workspace: &Workspace) {
    let mut changes = workspace.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(change) => {
                    let _ = app.emit("change", change);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = app.emit("change", shy_core::views::Change::Sync);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn install(app: &AppHandle, state: &AppState, workspace: Workspace) -> Session {
    forward_changes(app, &workspace);
    let session = session_of(&workspace);
    *state.workspace.lock().await = Some(workspace);
    session
}

async fn current(state: &State<'_, AppState>) -> CommandResult<Workspace> {
    state
        .workspace
        .lock()
        .await
        .clone()
        .ok_or_else(|| "no workspace is open".to_owned())
}

#[tauri::command]
async fn session(app: AppHandle, state: State<'_, AppState>) -> CommandResult<Option<Session>> {
    if let Some(workspace) = state.workspace.lock().await.as_ref() {
        return Ok(Some(session_of(workspace)));
    }
    if !Workspace::exists(&state.data.path) {
        return Ok(None);
    }
    let workspace = Workspace::open(&state.data, Networking::Online)
        .await
        .map_err(failed)?;
    Ok(Some(install(&app, &state, workspace).await))
}

#[tauri::command]
async fn create_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_name: String,
    name: String,
    bucket: BucketConfig,
) -> CommandResult<Session> {
    let profile = Profile {
        name: name.trim(),
        is_bot: false,
    };
    let workspace = Workspace::create(
        &state.data,
        workspace_name.trim(),
        profile,
        bucket,
        Networking::Online,
    )
    .await
    .map_err(failed)?;
    Ok(install(&app, &state, workspace).await)
}

#[tauri::command]
async fn join_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    invite: String,
    name: String,
) -> CommandResult<Session> {
    let profile = Profile {
        name: name.trim(),
        is_bot: false,
    };
    let workspace = Workspace::join(&state.data, &invite, profile, Networking::Online)
        .await
        .map_err(failed)?;
    Ok(install(&app, &state, workspace).await)
}

/// Shown while waiting to join, for the admin to compare.
#[tauri::command]
async fn safety_code(state: State<'_, AppState>) -> CommandResult<String> {
    state.data.safety_code().await.map_err(failed)
}

#[tauri::command]
async fn restore_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    kit: String,
) -> CommandResult<Session> {
    let workspace = Workspace::restore(&state.data, &kit, Networking::Online)
        .await
        .map_err(failed)?;
    Ok(install(&app, &state, workspace).await)
}

#[tauri::command]
async fn recovery_kit(state: State<'_, AppState>) -> CommandResult<String> {
    let kit = current(&state).await?.recovery_kit().await.map_err(failed)?;
    Ok(kit.as_str().to_owned())
}

/// Checks bucket settings before a workspace is created with them.
#[tauri::command]
async fn test_bucket(bucket: BucketConfig) -> CommandResult<()> {
    bucket.check().await.map_err(failed)
}

#[tauri::command]
async fn storage_settings(state: State<'_, AppState>) -> CommandResult<StorageSettings> {
    current(&state).await?.storage_settings().map_err(failed)
}

/// Returns the admin's new recovery kit, if they have one: it holds the keys.
#[tauri::command]
async fn update_storage(
    state: State<'_, AppState>,
    region: String,
    access_key_id: String,
    secret_access_key: String,
) -> CommandResult<Option<String>> {
    let workspace = current(&state).await?;
    let update = StorageUpdate {
        region: &region,
        access_key_id: &access_key_id,
        secret_access_key: &secret_access_key,
    };
    workspace.update_storage(update).await.map_err(failed)?;
    if !workspace.has_recovery_kit().map_err(failed)? {
        return Ok(None);
    }
    let kit = workspace.recovery_kit().await.map_err(failed)?;
    Ok(Some(kit.as_str().to_owned()))
}

#[tauri::command]
async fn channels(state: State<'_, AppState>) -> CommandResult<Vec<Channel>> {
    current(&state).await?.channels().map_err(failed)
}

#[tauri::command]
async fn create_channel(state: State<'_, AppState>, name: String) -> CommandResult<String> {
    current(&state).await?.create_channel(&name).map_err(failed)
}

#[tauri::command]
async fn messages(
    state: State<'_, AppState>,
    channel: String,
    before: Option<MessageCursor>,
    limit: u32,
) -> CommandResult<Vec<Message>> {
    current(&state)
        .await?
        .messages(&channel, before.as_ref(), limit)
        .map_err(failed)
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    channel: String,
    text: String,
    reply_to: Option<String>,
) -> CommandResult<String> {
    current(&state)
        .await?
        .send_message(&channel, &text, reply_to)
        .map_err(failed)
}

#[tauri::command]
async fn react(
    state: State<'_, AppState>,
    message: String,
    emoji: String,
    active: bool,
) -> CommandResult<()> {
    current(&state)
        .await?
        .react(&message, &emoji, active)
        .map_err(failed)
}

#[tauri::command]
async fn pages(state: State<'_, AppState>) -> CommandResult<Vec<Page>> {
    current(&state).await?.pages().map_err(failed)
}

#[tauri::command]
async fn create_page(
    state: State<'_, AppState>,
    title: String,
    parent: Option<String>,
) -> CommandResult<String> {
    current(&state).await?.create_page(&title, parent).map_err(failed)
}

#[tauri::command]
async fn rename_page(state: State<'_, AppState>, page: String, title: String) -> CommandResult<()> {
    current(&state).await?.rename_page(&page, &title).map_err(failed)
}

/// The page's full Yjs state as raw bytes (an ArrayBuffer in the UI).
#[tauri::command]
async fn page_state(state: State<'_, AppState>, page: String) -> CommandResult<Response> {
    let bytes = current(&state).await?.page_state(&page).map_err(failed)?;
    Ok(Response::new(bytes.unwrap_or_default()))
}

#[tauri::command]
async fn apply_doc_update(state: State<'_, AppState>, page: String, update: Vec<u8>) -> CommandResult<()> {
    current(&state)
        .await?
        .apply_doc_update(&page, update)
        .map_err(failed)
}

#[tauri::command]
async fn members(state: State<'_, AppState>) -> CommandResult<Vec<Member>> {
    current(&state).await?.members().map_err(failed)
}

#[tauri::command]
async fn remove_member(state: State<'_, AppState>, member: String) -> CommandResult<()> {
    current(&state)
        .await?
        .remove_member(&member)
        .await
        .map_err(failed)
}

#[tauri::command]
async fn invites(state: State<'_, AppState>) -> CommandResult<Vec<Invite>> {
    current(&state).await?.invites().map_err(failed)
}

#[tauri::command]
async fn create_invite(state: State<'_, AppState>, label: String) -> CommandResult<String> {
    current(&state).await?.create_invite(&label).await.map_err(failed)
}

#[tauri::command]
async fn approve_invite(state: State<'_, AppState>, token: String) -> CommandResult<()> {
    current(&state)
        .await?
        .approve_invite(&token)
        .await
        .map_err(failed)
}

#[tauri::command]
async fn reject_invite(state: State<'_, AppState>, token: String) -> CommandResult<()> {
    current(&state).await?.reject_invite(&token).await.map_err(failed)
}

#[tauri::command]
async fn revoke_invite(state: State<'_, AppState>, token: String) -> CommandResult<()> {
    current(&state).await?.revoke_invite(&token).await.map_err(failed)
}

#[tauri::command]
async fn search(state: State<'_, AppState>, query: String) -> CommandResult<Vec<SearchHit>> {
    current(&state).await?.search(&query, 30).map_err(failed)
}

#[tauri::command]
async fn status(state: State<'_, AppState>) -> CommandResult<SyncStatus> {
    Ok(current(&state).await?.status())
}

#[tauri::command]
async fn set_profile(state: State<'_, AppState>, name: String) -> CommandResult<()> {
    current(&state)
        .await?
        .set_profile(Profile {
            name: &name,
            is_bot: false,
        })
        .map_err(failed)
}

/// `SHY_DIR` lets several members run side by side on one machine.
fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    match std::env::var_os("SHY_DIR") {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => Ok(app.path().app_data_dir()?.join("workspace")),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = data_dir(app.handle())?;
            app.manage(AppState {
                data: DataDir::new(dir, KeyStorage::Keychain),
                workspace: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session,
            create_workspace,
            join_workspace,
            restore_workspace,
            test_bucket,
            storage_settings,
            update_storage,
            safety_code,
            recovery_kit,
            channels,
            create_channel,
            messages,
            send_message,
            react,
            pages,
            create_page,
            rename_page,
            page_state,
            apply_doc_update,
            members,
            remove_member,
            invites,
            create_invite,
            approve_invite,
            reject_invite,
            revoke_invite,
            search,
            status,
            set_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running shyspace");
}
