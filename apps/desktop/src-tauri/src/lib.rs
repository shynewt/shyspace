//! Thin bridge from the UI to shy-core: every command maps to one Workspace
//! call, and every core change is forwarded to the UI as a "change" event.

mod alert_actions;
mod alerts;
mod clipboard;
mod files;
mod huddles;
#[cfg(target_os = "linux")]
mod linux_autostart;
#[cfg(target_os = "macos")]
mod macos_launch;
mod notification_permission;
#[cfg(target_os = "macos")]
mod notify_macos;
mod open_at_login;
mod pages_live;
mod single_instance;
mod tray;
mod updates;
mod window;

use std::path::PathBuf;

use serde::Serialize;
use shy_core::{
    BucketConfig, DataDir, JoinCancel, KeyStorage, Networking, Profile, Workspace, WorkspaceOptions,
    profile::ProfileDetails,
    sync::SyncStatus,
    views::{
        Channel, Draft, DueReminder, Inbox, Invite, LaterItem, LinkedMessage, Member, Message, MessageCursor,
        NotifyLevel, OldKeyCheck, Page, PageEditorSession, PrivateMembers, ProtectionCheck, SearchHit,
        SectionSort, SidebarSection, StatusUpdate, StorageRotation, StorageSettings, WorkspaceBadge,
        WorkspaceItem, WorkspaceLook,
    },
    workspace::StorageUpdate,
    workspaces::{AppIcon, DeviceKeys, Workspaces},
};
use shy_media::ShyMedia;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent, ipc::Response};
use tokio::sync::Mutex;

pub(crate) type CommandResult<T> = Result<T, String>;

/// How long quitting may wait to upload unsent changes.
const EXIT_UPLOAD_LIMIT: std::time::Duration = std::time::Duration::from_secs(3);

pub(crate) struct AppState {
    workspaces: Workspaces,
    /// The one workspace folder of earlier versions, moved once on start.
    single_folder: Option<DataDir>,
    /// Held while opening workspaces, so two opens don't race.
    opening: Mutex<()>,
    /// Stops the join in progress, if one is waiting for the admin.
    joining: std::sync::Mutex<Option<JoinCancel>>,
    /// Huddle audio, shared by every workspace: one huddle at a time.
    media: ShyMedia,
}

/// A change in one workspace, or (with no id) in the set of workspaces.
#[derive(Serialize, Clone)]
struct WorkspaceChange {
    workspace_id: Option<String>,
    change: shy_core::views::Change,
}

impl AppState {
    pub(crate) fn open_workspaces(&self) -> Vec<Workspace> {
        self.workspaces.open_workspaces()
    }

    /// Online, with huddle audio served on the workspace's own endpoint.
    fn online(&self) -> WorkspaceOptions {
        let audio = self.media.for_workspace();
        WorkspaceOptions::from(Networking::Online)
            .with_protocols(audio.protocols())
            .with_media(audio.huddle_media())
    }
}

#[derive(Serialize)]
struct Session {
    workspace_id: String,
    workspace_name: String,
    me: String,
    is_admin: bool,
}

pub(crate) fn failed(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn session_of(workspace: &Workspace) -> Session {
    Session {
        workspace_id: workspace.workspace_id().to_owned(),
        workspace_name: workspace.name().to_owned(),
        me: workspace.my_id(),
        is_admin: workspace.is_admin(),
    }
}

/// Sends every change of `workspace` to the UI, tagged with its id.
fn forward_changes(app: &AppHandle, workspace: &Workspace) {
    let mut changes = workspace.subscribe();
    let workspace_id = Some(workspace.workspace_id().to_owned());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let change = match changes.recv().await {
                Ok(change) => change,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => shy_core::views::Change::Sync,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let tagged = WorkspaceChange {
                workspace_id: workspace_id.clone(),
                change,
            };
            let _ = app.emit("change", tagged);
        }
    });
}

/// Tells the UI the set of workspaces changed.
fn emit_workspaces_changed(app: &AppHandle) {
    let change = WorkspaceChange {
        workspace_id: None,
        change: shy_core::views::Change::Workspaces,
    };
    let _ = app.emit("change", change);
}

/// Starts showing a workspace just added to this device.
fn install(app: &AppHandle, workspace: &Workspace) -> Session {
    forward_changes(app, workspace);
    show_selected(app, workspace)
}

/// What the UI shows after the selected workspace changed.
fn show_selected(app: &AppHandle, workspace: &Workspace) -> Session {
    app.state::<files::StagedFiles>().clear();
    emit_workspaces_changed(app);
    session_of(workspace)
}

/// The selected workspace, which every command acts on.
pub(crate) async fn current(state: &State<'_, AppState>) -> CommandResult<Workspace> {
    state
        .workspaces
        .selected()
        .ok_or_else(|| "no workspace is open".to_owned())
}

#[tauri::command]
async fn session(app: AppHandle, state: State<'_, AppState>) -> CommandResult<Option<Session>> {
    open_workspaces(&app, &state).await
}

/// Opens the workspaces on this device that aren't open yet (moving the
/// single folder of earlier versions first), and returns what to show.
pub(crate) async fn open_workspaces(app: &AppHandle, state: &AppState) -> CommandResult<Option<Session>> {
    // Held across opening, so two calls at once (React runs effects twice in
    // development) wait for each other instead of both opening the folders.
    let _opening = state.opening.lock().await;
    if let Some(single_folder) = &state.single_folder {
        state
            .workspaces
            .adopt_single_folder(single_folder)
            .await
            .map_err(failed)?;
    }
    let opened = state
        .workspaces
        .open_all(|| state.online())
        .await
        .map_err(failed)?;
    for (workspace_id, error) in opened.failed {
        tracing::error!("couldn't open workspace {workspace_id}: {error:#}");
    }
    for workspace in &opened.workspaces {
        forward_changes(app, workspace);
    }
    Ok(state
        .workspaces
        .selected()
        .map(|workspace| session_of(&workspace)))
}

/// The workspaces on this device, in the order the person arranged them.
#[tauri::command]
fn workspaces(state: State<'_, AppState>) -> CommandResult<Vec<WorkspaceItem>> {
    state.workspaces.rail().map_err(failed)
}

/// Shows another workspace; it becomes the one every command acts on. One
/// that is closed (a leave that couldn't finish) is opened first.
#[tauri::command]
async fn select_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> CommandResult<Session> {
    if state.workspaces.find(&workspace_id).is_none() {
        open_workspaces(&app, &state).await?;
    }
    let workspace = state.workspaces.select(&workspace_id).map_err(failed)?;
    Ok(show_selected(&app, &workspace))
}

/// Marks everything read in any workspace, from the rail's menu.
#[tauri::command]
fn mark_workspace_read(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> CommandResult<()> {
    let workspace = state
        .workspaces
        .find(&workspace_id)
        .ok_or_else(|| "that workspace isn't open".to_owned())?;
    workspace.mark_all_read().map_err(failed)?;
    emit_workspaces_changed(&app);
    Ok(())
}

#[tauri::command]
fn move_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    position: usize,
) -> CommandResult<()> {
    state
        .workspaces
        .move_to(&workspace_id, position)
        .map_err(failed)?;
    emit_workspaces_changed(&app);
    Ok(())
}

/// Deletes a workspace from this device for good (the UI confirms first).
/// Returns what to show next, if any workspace is left.
#[tauri::command]
async fn leave_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> CommandResult<Option<Session>> {
    let shown_before = state
        .workspaces
        .selected()
        .map(|workspace| workspace.workspace_id().to_owned());
    let left = {
        // Not alongside an open, which could reopen the folder being deleted.
        let _opening = state.opening.lock().await;
        state.workspaces.leave(&workspace_id).await
    };
    if let Err(error) = left {
        // It may have closed before failing, so the shown one may differ.
        emit_workspaces_changed(&app);
        return Err(failed(error));
    }
    let Some(shown) = state.workspaces.selected() else {
        emit_workspaces_changed(&app);
        return Ok(None);
    };
    if shown_before.as_deref() == Some(shown.workspace_id()) {
        emit_workspaces_changed(&app);
        return Ok(Some(session_of(&shown)));
    }
    Ok(Some(show_selected(&app, &shown)))
}

/// What each workspace's rail icon shows.
#[tauri::command]
fn workspace_badges(state: State<'_, AppState>) -> CommandResult<Vec<WorkspaceBadge>> {
    state
        .workspaces
        .open_workspaces()
        .iter()
        .map(Workspace::badge)
        .collect::<anyhow::Result<_>>()
        .map_err(failed)
}

#[tauri::command]
fn set_workspace_muted(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    muted: bool,
) -> CommandResult<()> {
    match muted {
        true => state.workspaces.mute(&workspace_id),
        false => state.workspaces.unmute(&workspace_id),
    }
    .map_err(failed)?;
    emit_workspaces_changed(&app);
    Ok(())
}

/// Holds every workspace's notifications until `until_ms`, or lets them
/// show again with none.
#[tauri::command]
fn set_notifications_paused(state: State<'_, AppState>, until_ms: Option<i64>) -> CommandResult<()> {
    match until_ms {
        Some(until_ms) => state.workspaces.pause_notifications(until_ms),
        None => state.workspaces.resume_notifications(),
    }
    .map_err(failed)
}

#[tauri::command]
fn notifications_paused(state: State<'_, AppState>) -> CommandResult<Option<i64>> {
    state.workspaces.notifications_paused().map_err(failed)
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
    let workspace = {
        let _opening = state.opening.lock().await;
        state
            .workspaces
            .create(workspace_name.trim(), profile, bucket, state.online())
            .await
            .map_err(failed)?
    };
    Ok(install(&app, &workspace))
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
    let cancel = JoinCancel::new();
    {
        let mut joining = state
            .joining
            .lock()
            .map_err(|_| "join state lock poisoned".to_owned())?;
        if joining.is_some() {
            return Err("a join request is already waiting for the admin".to_owned());
        }
        *joining = Some(cancel.clone());
    }
    let joined = state
        .workspaces
        .join(&invite, profile, state.online(), &cancel)
        .await;
    *state
        .joining
        .lock()
        .map_err(|_| "join state lock poisoned".to_owned())? = None;
    Ok(install(&app, &joined.map_err(failed)?))
}

/// How the app shows itself outside its window, and on which system.
#[derive(Serialize)]
struct AppIconView {
    platform: &'static str,
    #[serde(flatten)]
    icon: AppIcon,
}

#[tauri::command]
fn app_icon(state: State<'_, AppState>) -> CommandResult<AppIconView> {
    Ok(AppIconView {
        platform: std::env::consts::OS,
        icon: state.workspaces.app_icon().map_err(failed)?,
    })
}

#[tauri::command]
fn set_in_menu_bar(app: AppHandle, state: State<'_, AppState>, is_shown: bool) -> CommandResult<()> {
    state.workspaces.set_in_menu_bar(is_shown).map_err(failed)?;
    tray::show_in_menu_bar(&app, is_shown);
    Ok(())
}

#[tauri::command]
fn set_shows_unread_count(app: AppHandle, state: State<'_, AppState>, is_shown: bool) -> CommandResult<()> {
    state
        .workspaces
        .set_shows_unread_count(is_shown)
        .map_err(failed)?;
    let count = app.state::<tray::Attention>().count();
    tray::show_attention(&app, count).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_tray_monochrome(app: AppHandle, state: State<'_, AppState>, is_monochrome: bool) -> CommandResult<()> {
    state
        .workspaces
        .set_tray_monochrome(is_monochrome)
        .map_err(failed)?;
    tray::redraw(&app);
    Ok(())
}

/// How many things need you, shown outside the window.
#[tauri::command]
fn set_attention_count(app: AppHandle, count: u32) -> CommandResult<()> {
    tray::show_attention(&app, count).map_err(|error| error.to_string())
}

/// Stops waiting for the admin to approve a join request.
#[tauri::command]
fn cancel_join(state: State<'_, AppState>) -> CommandResult<()> {
    let joining = state
        .joining
        .lock()
        .map_err(|_| "join state lock poisoned".to_owned())?;
    if let Some(cancel) = joining.as_ref() {
        cancel.cancel();
    }
    Ok(())
}

/// Shown while waiting to join with `invite`, for the admin to compare:
/// made in the folder that workspace will live in, before the join starts.
#[tauri::command]
async fn safety_code(state: State<'_, AppState>, invite: String) -> CommandResult<String> {
    let folder = state.workspaces.folder_for_invite(&invite).map_err(failed)?;
    folder.safety_code().await.map_err(failed)
}

#[tauri::command]
async fn restore_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    kit: String,
) -> CommandResult<Session> {
    let workspace = {
        let _opening = state.opening.lock().await;
        state
            .workspaces
            .restore(&kit, state.online())
            .await
            .map_err(failed)?
    };
    Ok(install(&app, &workspace))
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

#[tauri::command]
async fn storage_rotation(state: State<'_, AppState>) -> CommandResult<Option<StorageRotation>> {
    current(&state).await?.storage_rotation().await.map_err(failed)
}

#[tauri::command]
async fn check_old_key_deleted(state: State<'_, AppState>) -> CommandResult<OldKeyCheck> {
    current(&state)
        .await?
        .check_old_key_deleted()
        .await
        .map_err(failed)
}

#[tauri::command]
async fn cancel_storage_rotation(state: State<'_, AppState>) -> CommandResult<()> {
    current(&state).await?.cancel_storage_rotation().map_err(failed)
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
async fn create_private_channel(
    state: State<'_, AppState>,
    name: String,
    members: Vec<String>,
) -> CommandResult<String> {
    current(&state)
        .await?
        .create_private_channel(&name, &members)
        .map_err(failed)
}

#[tauri::command]
async fn open_dm(state: State<'_, AppState>, members: Vec<String>) -> CommandResult<String> {
    current(&state).await?.open_dm(&members).map_err(failed)
}

#[tauri::command]
async fn open_notes(state: State<'_, AppState>) -> CommandResult<String> {
    current(&state).await?.open_notes().map_err(failed)
}

#[tauri::command]
async fn private_members(state: State<'_, AppState>, channel: String) -> CommandResult<PrivateMembers> {
    current(&state).await?.private_members(&channel).map_err(failed)
}

#[tauri::command]
async fn add_to_private_channel(
    state: State<'_, AppState>,
    channel: String,
    member: String,
) -> CommandResult<()> {
    current(&state)
        .await?
        .add_to_private_channel(&channel, &member)
        .map_err(failed)
}

#[tauri::command]
async fn remove_from_private_channel(
    state: State<'_, AppState>,
    channel: String,
    member: String,
) -> CommandResult<()> {
    current(&state)
        .await?
        .remove_from_private_channel(&channel, &member)
        .map_err(failed)
}

#[tauri::command]
async fn hand_over_private_channel(
    state: State<'_, AppState>,
    channel: String,
    member: String,
) -> CommandResult<()> {
    current(&state)
        .await?
        .hand_over_private_channel(&channel, &member)
        .map_err(failed)
}

#[tauri::command]
async fn leave_private_channel(state: State<'_, AppState>, channel: String) -> CommandResult<()> {
    current(&state)
        .await?
        .leave_private_channel(&channel)
        .map_err(failed)
}

#[tauri::command]
async fn rotate_private_channel_key(state: State<'_, AppState>, channel: String) -> CommandResult<()> {
    current(&state)
        .await?
        .rotate_private_channel_key(&channel)
        .map_err(failed)
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

/// Shows a desktop notification. The UI decides when one is wanted; a
/// muted workspace, or a pause on all, holds it back.
#[tauri::command]
fn show_alert(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: Option<String>,
    title: String,
    body: String,
    target: Option<alerts::AlertTarget>,
    picture: Option<alerts::AlertPicture>,
) -> CommandResult<()> {
    let may_notify = match &workspace_id {
        Some(workspace_id) => state.workspaces.may_notify(workspace_id),
        None => state
            .workspaces
            .notifications_paused()
            .map(|paused| paused.is_none()),
    }
    .map_err(failed)?;
    if !may_notify {
        return Ok(());
    }
    alerts::show(&app, workspace_id, title, body, target, picture)
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
async fn mark_page_mentions_read(state: State<'_, AppState>, page: String) -> CommandResult<()> {
    current(&state)
        .await?
        .mark_page_mentions_read(&page)
        .map_err(failed)
}

#[tauri::command]
async fn linked_message(state: State<'_, AppState>, id: String) -> CommandResult<Option<LinkedMessage>> {
    current(&state).await?.linked_message(&id).map_err(failed)
}

#[tauri::command]
async fn sent_messages(
    state: State<'_, AppState>,
    text: String,
    before: Option<MessageCursor>,
    limit: u32,
) -> CommandResult<Vec<LinkedMessage>> {
    current(&state)
        .await?
        .sent_messages(&text, before.as_ref(), limit)
        .map_err(failed)
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
/// Reminders that came due in any workspace, each with its workspace.
async fn take_due_reminders(state: State<'_, AppState>) -> CommandResult<Vec<DueReminder>> {
    state.workspaces.take_due_reminders().map_err(failed)
}

/// When the next reminder in any workspace comes due, for one app-wide timer.
#[tauri::command]
fn next_reminder_ms(state: State<'_, AppState>) -> CommandResult<Option<i64>> {
    state.workspaces.next_reminder_ms().map_err(failed)
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

/// The shown workspace's default look, as its admin set it.
#[tauri::command]
async fn workspace_look(state: State<'_, AppState>) -> CommandResult<WorkspaceLook> {
    current(&state).await?.workspace_look().map_err(failed)
}

/// Sets the shown workspace's default look; `None` leaves the app's
/// default. Admin only.
#[tauri::command]
async fn set_workspace_look(
    state: State<'_, AppState>,
    theme: Option<String>,
    accent: Option<String>,
) -> CommandResult<()> {
    current(&state)
        .await?
        .set_workspace_look(theme.as_deref(), accent.as_deref())
        .map_err(failed)
}

#[tauri::command]
/// Away or back is about the person, so it shows in every workspace.
async fn set_away(state: State<'_, AppState>, away: bool) -> CommandResult<()> {
    state.workspaces.set_away(away);
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
async fn page_editor_session(state: State<'_, AppState>, page: String) -> CommandResult<PageEditorSession> {
    Ok(current(&state).await?.page_editor_session(&page))
}

#[tauri::command]
async fn apply_doc_update(
    state: State<'_, AppState>,
    page: String,
    nonce: String,
    update: Vec<u8>,
) -> CommandResult<()> {
    current(&state)
        .await?
        .apply_doc_update(&page, &nonce, update)
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

#[tauri::command]
async fn set_profile_details(state: State<'_, AppState>, details: ProfileDetails) -> CommandResult<()> {
    current(&state)
        .await?
        .set_profile_details(&details)
        .map_err(failed)
}

/// The workspaces this app holds: every one in the app's data folder, or
/// (with `SHY_DIR`, to run several members side by side on one machine)
/// just the one in that folder, as `shy --dir` uses it.
fn workspaces_on_this_device(app: &AppHandle) -> tauri::Result<(Workspaces, Option<DataDir>)> {
    if let Some(dir) = std::env::var_os("SHY_DIR") {
        return Ok((
            Workspaces::in_one_folder(PathBuf::from(dir), DeviceKeys::Keychain),
            None,
        ));
    }
    let root = app.path().app_data_dir()?;
    let single_folder = DataDir::new(root.join("workspace"), KeyStorage::Keychain);
    Ok((Workspaces::new(root, DeviceKeys::Keychain), Some(single_folder)))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            // The updater logs every error status of the feed, a missing one
            // included; `updates` reports the failures that matter.
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,tauri_plugin_updater=off".into()),
        )
        .init();
    let mut builder = tauri::Builder::default();
    if let Some(single_instance) = single_instance::plugin() {
        builder = builder.plugin(single_instance);
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(files::StagedFiles::default())
        .setup(|app| {
            let (workspaces, single_folder) = workspaces_on_this_device(app.handle())?;
            app.manage(AppState {
                workspaces,
                single_folder,
                opening: Mutex::new(()),
                joining: std::sync::Mutex::new(None),
                media: ShyMedia::new(),
            });
            window::create_main_window(app.handle())?;
            alerts::follow_clicks(app.handle())?;
            app.manage(tray::HasTray::default());
            app.manage(tray::Attention::default());
            tray::install_at_start(app.handle());
            open_at_login::refresh_at_start();
            let has_tray = open_at_login::is_tray_seen(
                app.state::<tray::HasTray>().get(),
                &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            );
            let is_at_login = open_at_login::was_started_at_login();
            if !open_at_login::starts_hidden(is_at_login, cfg!(target_os = "macos"), has_tray) {
                tray::show_main_window(app.handle());
            }
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            app.manage(updates::PendingUpdate::default());
            Ok(())
        })
        // With a tray icon, closing the window keeps shyspace running there,
        // so alerts still arrive; Quit in the tray menu ends it.
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(drop) = event {
                let scale_factor = match window.scale_factor() {
                    Ok(scale) => Some(scale),
                    Err(error) => {
                        tracing::warn!(%error, "couldn't read the window's scale; a drop goes to the box used last");
                        None
                    }
                };
                files::on_drag_drop(
                    window.app_handle(),
                    &window.state::<files::StagedFiles>(),
                    drop,
                    scale_factor,
                );
            }
            if let WindowEvent::ThemeChanged(_) = event {
                tray::redraw(window.app_handle());
            }
            // On macOS the app stays in the Dock, as Mac apps do: closing the
            // window hides it, the Dock icon brings it back, Quit ends it.
            if let WindowEvent::CloseRequested { api, .. } = event
                && (cfg!(target_os = "macos") || window.state::<tray::HasTray>().get())
            {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    tracing::warn!("could not hide the window: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            updates::check_for_update,
            updates::restart_to_update,
            huddles::huddle,
            huddles::my_huddle,
            huddles::join_huddle,
            huddles::leave_huddle,
            huddles::decline_huddle,
            huddles::huddle_ringing,
            huddles::set_huddle_muted,
            huddles::huddle_devices,
            huddles::set_huddle_devices,
            huddles::huddle_sounds,
            huddles::set_huddle_sounds,
            files::pick_files,
            files::stage_pasted_file,
            clipboard::clipboard_image,
            files::staged_file_bytes,
            files::unstage_file,
            files::send_files,
            files::file_bytes,
            files::share_page_file,
            files::page_files,
            pages_live::join_page,
            pages_live::leave_page,
            pages_live::move_cursor,
            pages_live::page_presence,
            files::page_file_bytes,
            files::save_page_file,
            files::save_file,
            files::delete_file,
            files::rename_file,
            files::files,
            files::profile_picture,
            files::set_profile_picture,
            files::workspace_icon,
            files::set_workspace_icon,
            workspace_look,
            set_workspace_look,
            session,
            create_workspace,
            join_workspace,
            restore_workspace,
            test_bucket,
            storage_settings,
            storage_protection,
            check_storage_protection,
            update_storage,
            workspaces,
            select_workspace,
            move_workspace,
            leave_workspace,
            mark_workspace_read,
            next_reminder_ms,
            workspace_badges,
            set_workspace_muted,
            set_notifications_paused,
            notifications_paused,
            storage_rotation,
            check_old_key_deleted,
            cancel_storage_rotation,
            safety_code,
            recovery_kit,
            channels,
            create_channel,
            create_private_channel,
            open_dm,
            open_notes,
            cancel_join,
            set_attention_count,
            app_icon,
            set_in_menu_bar,
            set_shows_unread_count,
            set_tray_monochrome,
            alert_actions::take_alert_actions,
            alert_actions::people_in,
            alert_actions::reply_to_alert,
            alert_actions::keep_unsent_reply,
            alert_actions::mark_alert_read,
            notification_permission::notification_permission,
            notification_permission::request_notification_permission,
            notification_permission::open_notification_settings,
            open_at_login::open_at_login,
            open_at_login::set_open_at_login,
            private_members,
            add_to_private_channel,
            remove_from_private_channel,
            hand_over_private_channel,
            leave_private_channel,
            rotate_private_channel_key,
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
            sent_messages,
            linked_message,
            inbox,
            mark_all_read,
            mark_page_mentions_read,
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
            page_editor_session,
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
            set_profile_details,
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
    tauri::async_runtime::block_on(async {
        let workspaces = app.state::<AppState>().workspaces.close_all();
        shut_down(workspaces).await;
    });
}

/// Shuts workspaces down together, uploading what wasn't sent yet, within
/// `EXIT_UPLOAD_LIMIT`. What doesn't make it stays on disk and goes at the
/// next start.
pub(crate) async fn shut_down(workspaces: Vec<Workspace>) {
    let mut shutdowns = tokio::task::JoinSet::new();
    for workspace in &workspaces {
        let workspace = workspace.clone();
        shutdowns.spawn(async move { workspace.shutdown().await });
    }
    let finished = tokio::time::timeout(EXIT_UPLOAD_LIMIT, async {
        while let Some(shutdown) = shutdowns.join_next().await {
            match shutdown {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!("could not upload unsent changes before closing: {error:#}"),
                Err(error) => tracing::warn!("uploading before closing stopped: {error}"),
            }
        }
    })
    .await;
    if finished.is_err() {
        tracing::warn!("gave up uploading unsent changes: closing took too long");
        for workspace in &workspaces {
            workspace.stop_background();
        }
    }
}

#[cfg(test)]
mod tests {
    /// `unsafe_code` is only denied in this crate, not forbidden, so that
    /// `notify_macos.rs` can call Objective-C; no other file may allow it.
    #[test]
    fn only_the_macos_notification_module_allows_unsafe_code() {
        // Split, so this test doesn't find itself.
        let allow = concat!("allow(", "unsafe_code)");
        let sources = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut allowing = Vec::new();
        for entry in std::fs::read_dir(&sources).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            if text.contains(allow) {
                allowing.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
        assert_eq!(allowing, ["notify_macos.rs"]);
    }
}
