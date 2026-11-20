//! Thin bridge from the UI to shy-core: every command maps to one Workspace
//! call, and every core change is forwarded to the UI as a "change" event.

mod tray;

use std::path::PathBuf;

use serde::Serialize;
use shy_core::{
    BucketConfig, DataDir, KeyStorage, Networking, Profile, Workspace,
    sync::SyncStatus,
    views::{
        Channel, Draft, Inbox, Invite, LaterItem, LinkedMessage, Member, Message, MessageCursor, NotifyLevel,
        Page, ProtectionCheck, SearchHit, SectionSort, SidebarSection, StatusUpdate, StorageSettings,
    },
    workspace::StorageUpdate,
};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, ipc::Response};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

type CommandResult<T> = Result<T, String>;

/// How long quitting may wait to upload unsent changes.
const EXIT_UPLOAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(3);

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
    activate(app, &mut *state.workspace.lock().await, workspace)
}

/// Makes `workspace` the open one and starts forwarding its changes to the UI.
fn activate(app: &AppHandle, slot: &mut Option<Workspace>, workspace: Workspace) -> Session {
    forward_changes(app, &workspace);
    let session = session_of(&workspace);
    *slot = Some(workspace);
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
    // Held across the open, so two calls at once (React runs effects twice in
    // development) wait for each other instead of both opening the folder.
    let mut slot = state.workspace.lock().await;
    if let Some(workspace) = slot.as_ref() {
        return Ok(Some(session_of(workspace)));
    }
    if !Workspace::exists(&state.data.path) {
        return Ok(None);
    }
    let workspace = Workspace::open(&state.data, Networking::Online)
        .await
        .map_err(failed)?;
    Ok(Some(activate(&app, &mut slot, workspace)))
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

#[tauri::command]
async fn storage_protection(state: State<'_, AppState>) -> CommandResult<ProtectionCheck> {
    current(&state).await?.storage_protection().map_err(failed)
}

#[tauri::command]
async fn check_storage_protection(state: State<'_, AppState>) -> CommandResult<ProtectionCheck> {
    current(&state)
        .await?
        .check_storage_protection()
        .await
        .map_err(failed)
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
async fn mark_read(state: State<'_, AppState>, channel: String, message: String) -> CommandResult<()> {
    current(&state)
        .await?
        .mark_read(&channel, &message)
        .map_err(failed)
}

#[tauri::command]
async fn set_notify_level(
    state: State<'_, AppState>,
    channel: String,
    level: NotifyLevel,
) -> CommandResult<()> {
    current(&state)
        .await?
        .set_notify_level(&channel, level)
        .map_err(failed)
}

/// Shows a desktop notification. The UI decides when one is wanted.
#[tauri::command]
fn show_alert(app: AppHandle, title: String, body: String) -> CommandResult<()> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn thread(state: State<'_, AppState>, root: String) -> CommandResult<Vec<Message>> {
    current(&state).await?.thread(&root).map_err(failed)
}

#[tauri::command]
async fn set_following(state: State<'_, AppState>, root: String, following: bool) -> CommandResult<()> {
    current(&state)
        .await?
        .set_following(&root, following)
        .map_err(failed)
}

#[tauri::command]
async fn mark_thread_read(state: State<'_, AppState>, root: String, message: String) -> CommandResult<()> {
    current(&state)
        .await?
        .mark_thread_read(&root, &message)
        .map_err(failed)
}

#[tauri::command]
async fn inbox(state: State<'_, AppState>) -> CommandResult<Inbox> {
    current(&state).await?.inbox().map_err(failed)
}

#[tauri::command]
async fn mark_all_read(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.mark_all_read().map_err(failed)
}

#[tauri::command]
async fn linked_message(state: State<'_, AppState>, id: String) -> CommandResult<Option<LinkedMessage>> {
    current(&state).await?.linked_message(&id).map_err(failed)
}

#[tauri::command]
async fn set_draft(state: State<'_, AppState>, target: String, text: String) -> CommandResult<()> {
    current(&state).await?.set_draft(&target, &text).map_err(failed)
}

#[tauri::command]
async fn drafts(state: State<'_, AppState>) -> CommandResult<Vec<Draft>> {
    current(&state).await?.drafts().map_err(failed)
}

#[tauri::command]
async fn save_for_later(
    state: State<'_, AppState>,
    item: String,
    remind_ms: Option<i64>,
) -> CommandResult<()> {
    current(&state)
        .await?
        .save_for_later(&item, remind_ms)
        .map_err(failed)
}

#[tauri::command]
async fn remove_from_later(state: State<'_, AppState>, item: String) -> CommandResult<()> {
    current(&state).await?.remove_from_later(&item).map_err(failed)
}

#[tauri::command]
async fn mark_later_done(state: State<'_, AppState>, item: String, done: bool) -> CommandResult<()> {
    current(&state)
        .await?
        .mark_later_done(&item, done)
        .map_err(failed)
}

#[tauri::command]
async fn later(state: State<'_, AppState>) -> CommandResult<Vec<LaterItem>> {
    current(&state).await?.later().map_err(failed)
}

#[tauri::command]
async fn take_due_reminders(state: State<'_, AppState>) -> CommandResult<Vec<LaterItem>> {
    current(&state).await?.take_due_reminders().map_err(failed)
}

#[tauri::command]
async fn sidebar_sections(state: State<'_, AppState>) -> CommandResult<Vec<SidebarSection>> {
    current(&state).await?.sidebar_sections().map_err(failed)
}

#[tauri::command]
async fn create_section(state: State<'_, AppState>, name: String) -> CommandResult<String> {
    current(&state).await?.create_section(&name).map_err(failed)
}

#[tauri::command]
async fn rename_section(state: State<'_, AppState>, section: String, name: String) -> CommandResult<()> {
    current(&state)
        .await?
        .rename_section(&section, &name)
        .map_err(failed)
}

#[tauri::command]
async fn delete_section(state: State<'_, AppState>, section: String) -> CommandResult<()> {
    current(&state).await?.delete_section(&section).map_err(failed)
}

#[tauri::command]
async fn set_section_sort(
    state: State<'_, AppState>,
    section: String,
    sort: SectionSort,
) -> CommandResult<()> {
    current(&state)
        .await?
        .set_section_sort(&section, sort)
        .map_err(failed)
}

#[tauri::command]
async fn set_section_collapsed(
    state: State<'_, AppState>,
    section: String,
    collapsed: bool,
) -> CommandResult<()> {
    current(&state)
        .await?
        .set_section_collapsed(&section, collapsed)
        .map_err(failed)
}

#[tauri::command]
async fn move_channel(
    state: State<'_, AppState>,
    channel: String,
    section: Option<String>,
) -> CommandResult<()> {
    current(&state)
        .await?
        .move_channel(&channel, section.as_deref())
        .map_err(failed)
}

#[tauri::command]
async fn set_status(
    state: State<'_, AppState>,
    emoji: String,
    text: String,
    until_ms: Option<i64>,
    alerts: NotifyLevel,
) -> CommandResult<()> {
    current(&state)
        .await?
        .set_status(StatusUpdate {
            emoji: &emoji,
            text: &text,
            until_ms,
            alerts,
        })
        .map_err(failed)
}

#[tauri::command]
async fn clear_status(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.clear_status().map_err(failed)
}

#[tauri::command]
async fn set_away(state: State<'_, AppState>, away: bool) -> CommandResult<()> {
    current(&state).await?.set_away(away);
    Ok(())
}

#[tauri::command]
async fn appear_offline(state: State<'_, AppState>) -> CommandResult<bool> {
    current(&state).await?.appear_offline().map_err(failed)
}

#[tauri::command]
async fn set_appear_offline(state: State<'_, AppState>, offline: bool) -> CommandResult<()> {
    current(&state).await?.set_appear_offline(offline).map_err(failed)
}

#[tauri::command]
async fn rename_channel(state: State<'_, AppState>, channel: String, name: String) -> CommandResult<()> {
    current(&state)
        .await?
        .rename_channel(&channel, &name)
        .map_err(failed)
}

#[tauri::command]
async fn delete_channel(state: State<'_, AppState>, channel: String) -> CommandResult<()> {
    current(&state).await?.delete_channel(&channel).map_err(failed)
}

#[tauri::command]
async fn edit_message(state: State<'_, AppState>, message: String, text: String) -> CommandResult<()> {
    current(&state)
        .await?
        .edit_message(&message, &text)
        .map_err(failed)
}

#[tauri::command]
async fn delete_message(state: State<'_, AppState>, message: String) -> CommandResult<()> {
    current(&state).await?.delete_message(&message).map_err(failed)
}

#[tauri::command]
async fn delete_page(state: State<'_, AppState>, page: String) -> CommandResult<()> {
    current(&state).await?.delete_page(&page).map_err(failed)
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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = data_dir(app.handle())?;
            app.manage(AppState {
                data: DataDir::new(dir, KeyStorage::Keychain),
                workspace: Mutex::new(None),
            });
            let has_tray = tray::try_install(app.handle());
            app.manage(tray::HasTray(has_tray));
            Ok(())
        })
        // With a tray icon, closing the window keeps shyspace running there,
        // so alerts still arrive; Quit in the tray menu ends it.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.state::<tray::HasTray>().0
            {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::warn!("could not hide the window: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            session,
            create_workspace,
            join_workspace,
            restore_workspace,
            test_bucket,
            storage_settings,
            storage_protection,
            check_storage_protection,
            update_storage,
            safety_code,
            recovery_kit,
            channels,
            create_channel,
            rename_channel,
            mark_read,
            set_status,
            clear_status,
            set_away,
            appear_offline,
            set_appear_offline,
            sidebar_sections,
            create_section,
            rename_section,
            delete_section,
            set_section_sort,
            set_section_collapsed,
            move_channel,
            save_for_later,
            remove_from_later,
            mark_later_done,
            later,
            take_due_reminders,
            set_draft,
            drafts,
            linked_message,
            inbox,
            mark_all_read,
            thread,
            set_following,
            mark_thread_read,
            set_notify_level,
            show_alert,
            delete_channel,
            edit_message,
            delete_message,
            messages,
            send_message,
            react,
            pages,
            create_page,
            rename_page,
            delete_page,
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
        .build(tauri::generate_context!())
        .expect("error while building shyspace")
        .run(on_run_event);
}

fn on_run_event(app: &AppHandle, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::Exit => upload_before_exit(app),
        // macOS: clicking the dock icon brings the hidden window back.
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => tray::show_main_window(app),
        _ => {}
    }
}

/// Sends what wasn't uploaded yet, so a message written just before quitting
/// isn't held back until the next start. Quitting never waits longer than
/// `EXIT_UPLOAD_LIMIT`.
fn upload_before_exit(app: &AppHandle) {
    let state = app.state::<AppState>();
    tauri::async_runtime::block_on(async {
        let Some(workspace) = state.workspace.lock().await.clone() else {
            return;
        };
        match tokio::time::timeout(EXIT_UPLOAD_LIMIT, workspace.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("could not upload unsent changes before quitting: {error:#}"),
            Err(_) => tracing::warn!("gave up uploading unsent changes: quitting took too long"),
        }
    });
}
