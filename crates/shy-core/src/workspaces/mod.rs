//! The workspaces on this device. Each lives in its own folder,
//! `workspaces/<workspace id>/`, with its own device key, identity and
//! connections, so two of them share nothing and can't tell they share a
//! person. `workspaces.json` keeps this device's choices about them: their
//! order, which were used last, which are muted, and a pause on all
//! notifications. A device from before there could be several has one
//! folder, moved here once (`adopt_single_folder`).
//!
//! All of them stay open, with gossip delivering live; the few used most
//! recently also check the bucket at full pace, the rest every few minutes
//! (`sync::Cadence`), so idle workspaces cost few requests.

mod linking;

pub use linking::ResumedLinkStep;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};

use crate::{
    BucketConfig, DataDir, JoinCancel, Profile, Workspace, WorkspaceOptions,
    config::WorkspaceConfig,
    device_settings::{DeviceSettings, DeviceSettingsFile},
    keys::{HostKeyStore, KeyStorage},
    membership::{invite::Invite, recovery::kit_workspace_id},
    ops::{new_random_id, now_ms},
    sync::Cadence,
    transport::bucket::{Connector, s3_connector},
    views::{DueReminder, WorkspaceHuddle, WorkspaceItem, WorkspaceRing},
};

const FOLDERS: &str = "workspaces";
const SETTINGS_FILE: &str = "workspaces.json";
const MAX_ID_LENGTH: usize = 64;
/// How many of the most recently used workspaces check the bucket at full pace.
pub const LIVE_WORKSPACES: usize = 5;
/// How long leaving waits for a closed workspace's tasks to let go of it.
#[cfg(not(test))]
const CLOSING_WAIT: Duration = Duration::from_secs(10);
#[cfg(test)]
const CLOSING_WAIT: Duration = Duration::from_millis(500);

/// Where device keys live.
#[derive(Clone)]
pub enum DeviceKeys {
    /// The OS keychain, one entry per workspace folder.
    Keychain,
    /// One file per workspace in this folder, for servers without a
    /// keychain. Keep it outside the data folder, or it protects nothing.
    Files(PathBuf),
    /// The hosting app's keystore, one account per workspace
    /// (`device-key-<workspace id>`). A store serves one data folder only:
    /// two folders on one store would share each workspace's key.
    Host(Arc<dyn HostKeyStore>),
}

/// Never shows what a store holds.
impl std::fmt::Debug for DeviceKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keychain => formatter.write_str("Keychain"),
            Self::Files(folder) => formatter.debug_tuple("Files").field(folder).finish(),
            Self::Host(_) => formatter.write_str("Host"),
        }
    }
}

/// What `open_all` opened, and what failed to open, with why.
#[derive(Default)]
pub struct Opened {
    pub workspaces: Vec<Workspace>,
    pub failed: Vec<(String, anyhow::Error)>,
}

/// How the app shows itself outside its window on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct AppIcon {
    /// A menu bar item on macOS (Linux and Windows always have a tray icon).
    pub is_in_menu_bar: bool,
    /// The unread count on the Dock or taskbar icon.
    pub shows_unread_count: bool,
    /// The tray icon as a one-colour glyph instead of the app icon (Linux,
    /// Windows); the macOS menu bar item is always one.
    pub is_tray_monochrome: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub workspace_id: String,
    pub name: String,
}

/// A workspace id names a folder, and comes from invites and recovery kits
/// someone else made, so only plain lowercase names are accepted: never a
/// path, a device name like `CON`, or two ids one case-insensitive disk
/// would take for the same folder.
fn is_plain_id(workspace_id: &str) -> bool {
    !workspace_id.is_empty()
        && workspace_id.len() <= MAX_ID_LENGTH
        && workspace_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_')
        && !is_windows_device_name(workspace_id)
}

/// Names Windows keeps for devices in every folder, whatever their case.
fn is_windows_device_name(name: &str) -> bool {
    if matches!(name, "con" | "prn" | "aux" | "nul") {
        return true;
    }
    let numbered = name.strip_prefix("com").or_else(|| name.strip_prefix("lpt"));
    matches!(numbered, Some(digit) if digit.len() == 1 && digit.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Where the workspaces' folders are.
enum Layout {
    /// `workspaces/<workspace id>/` under the app's data folder.
    Folders(PathBuf),
    /// One workspace folder, as `shy --dir` uses, to run several members
    /// side by side on one machine.
    OneFolder(PathBuf),
}

impl Layout {
    fn folder_of(&self, workspace_id: &str) -> PathBuf {
        match self {
            Self::Folders(root) => root.join(FOLDERS).join(workspace_id),
            Self::OneFolder(dir) => dir.clone(),
        }
    }

    /// The folder new workspace folders go into.
    fn parent(&self) -> PathBuf {
        match self {
            Self::Folders(root) => root.join(FOLDERS),
            Self::OneFolder(dir) => dir.parent().map(Path::to_path_buf).unwrap_or_default(),
        }
    }

    fn settings_file(&self) -> PathBuf {
        match self {
            Self::Folders(root) => root.join(SETTINGS_FILE),
            Self::OneFolder(dir) => {
                let name = dir.file_name().unwrap_or_default().to_string_lossy();
                self.parent().join(format!("{name}.{SETTINGS_FILE}"))
            }
        }
    }
}

pub struct Workspaces {
    layout: Layout,
    keys: DeviceKeys,
    connect: Connector,
    /// The open workspaces, the selected one first, then by last use.
    open: Mutex<Vec<Workspace>>,
    /// `workspaces.json`, shared with the workspaces for the choices that are
    /// the device's (its huddle devices).
    settings_file: Arc<DeviceSettingsFile>,
    /// This device's link to a person, if one is in progress.
    link: Mutex<linking::LinkSlot>,
    /// Links reach this bucket directly, where the app uses presigned links.
    #[cfg(test)]
    test_link_bucket: Mutex<Option<crate::transport::bucket::Bucket>>,
}

impl Workspaces {
    /// The workspaces kept under `root` (the app's data folder).
    pub fn new(root: impl Into<PathBuf>, keys: DeviceKeys) -> Self {
        Self::with_connector(root, keys, s3_connector())
    }

    /// The workspaces kept under `root`, all on one bucket kept in memory.
    #[cfg(feature = "memory-bucket")]
    pub fn with_memory_bucket(root: impl Into<PathBuf>, keys: DeviceKeys) -> Self {
        Self::with_connector(root, keys, crate::transport::bucket::memory_connector())
    }

    pub(crate) fn with_connector(root: impl Into<PathBuf>, keys: DeviceKeys, connect: Connector) -> Self {
        Self::laid_out(Layout::Folders(root.into()), keys, connect)
    }

    /// Just the workspace in `dir` (one it holds, or one made there).
    pub fn in_one_folder(dir: impl Into<PathBuf>, keys: DeviceKeys) -> Self {
        Self::in_one_folder_with_connector(dir, keys, s3_connector())
    }

    pub(crate) fn in_one_folder_with_connector(
        dir: impl Into<PathBuf>,
        keys: DeviceKeys,
        connect: Connector,
    ) -> Self {
        Self::laid_out(Layout::OneFolder(dir.into()), keys, connect)
    }

    fn laid_out(layout: Layout, keys: DeviceKeys, connect: Connector) -> Self {
        let settings_file = Arc::new(DeviceSettingsFile::new(layout.settings_file()));
        Self {
            layout,
            keys,
            connect,
            open: Mutex::default(),
            settings_file,
            link: Mutex::default(),
            #[cfg(test)]
            test_link_bucket: Mutex::default(),
        }
    }

    /// `options` with this device's settings, so the workspace keeps its
    /// device-wide choices there.
    fn with_device_settings(&self, options: impl Into<WorkspaceOptions>) -> WorkspaceOptions {
        let mut options = options.into();
        options.device_settings = Some(self.settings_file.clone());
        options
    }

    /// The folder and device key of `workspace_id`'s workspace.
    pub fn folder(&self, workspace_id: &str) -> Result<DataDir> {
        ensure!(is_plain_id(workspace_id), "{workspace_id:?} isn't a workspace id");
        let keys = match &self.keys {
            DeviceKeys::Keychain => KeyStorage::Keychain,
            DeviceKeys::Files(folder) => KeyStorage::File(folder.join(format!("{workspace_id}.key"))),
            DeviceKeys::Host(store) => KeyStorage::Host {
                store: store.clone(),
                account: format!("device-key-{workspace_id}"),
            },
        };
        Ok(DataDir::new(self.layout.folder_of(workspace_id), keys))
    }

    /// Every complete workspace here, in the order the person arranged them;
    /// ones not placed yet come last, by name.
    pub fn list(&self) -> Result<Vec<WorkspaceEntry>> {
        if let Layout::OneFolder(dir) = &self.layout {
            let peeked = WorkspaceConfig::peek(dir)?;
            return Ok(peeked
                .map(|(workspace_id, name)| WorkspaceEntry { workspace_id, name })
                .into_iter()
                .collect());
        }
        let folders = self.layout.parent();
        let entries = match std::fs::read_dir(&folders) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", folders.display())),
        };
        let mut found = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            // One broken folder mustn't hide the others.
            let (workspace_id, name) = match WorkspaceConfig::peek(&path) {
                Ok(Some(peeked)) => peeked,
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!("skipping {}: {error:#}", path.display());
                    continue;
                }
            };
            // A folder named after another workspace isn't one we made.
            if path.file_name().and_then(|name| name.to_str()) == Some(workspace_id.as_str()) {
                found.push(WorkspaceEntry { workspace_id, name });
            }
        }
        let order = self.settings()?.workspace_ids;
        found.sort_by(|left, right| {
            let place = |entry: &WorkspaceEntry| {
                order
                    .iter()
                    .position(|id| *id == entry.workspace_id)
                    .unwrap_or(usize::MAX)
            };
            place(left).cmp(&place(right)).then(left.name.cmp(&right.name))
        });
        Ok(found)
    }

    /// Moves `workspace_id` to `position` in the order (or last, past the end).
    pub fn move_to(&self, workspace_id: &str, position: usize) -> Result<()> {
        let mut ids: Vec<String> = self.list()?.into_iter().map(|entry| entry.workspace_id).collect();
        ensure!(ids.iter().any(|id| id == workspace_id), "no such workspace here");
        ids.retain(|id| id != workspace_id);
        ids.insert(position.min(ids.len()), workspace_id.to_owned());
        self.update_settings(|settings| {
            settings.workspace_ids = ids;
        })
    }

    /// Creates a workspace in a new folder of its own.
    pub async fn create(
        &self,
        workspace_name: &str,
        profile: Profile<'_>,
        bucket: BucketConfig,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let workspace_id = new_random_id();
        let data = self.prepare_folder(&workspace_id)?;
        let workspace = Workspace::create_with(
            &data,
            workspace_id.clone(),
            workspace_name,
            profile,
            bucket,
            self.connect.clone(),
            self.with_device_settings(options),
        )
        .await?;
        self.add_open(workspace)
    }

    /// Joins with an invite, into a new folder of its own.
    pub async fn join(
        &self,
        invite_code: &str,
        profile: Profile<'_>,
        options: impl Into<WorkspaceOptions>,
        cancel: &JoinCancel,
    ) -> Result<Workspace> {
        let workspace_id = Invite::decode(invite_code)?.workspace_id;
        let data = self.prepare_folder(&workspace_id)?;
        let workspace = Workspace::join_cancellable(
            &data,
            invite_code,
            profile,
            self.with_device_settings(options),
            cancel,
        )
        .await?;
        self.add_open(workspace)
    }

    /// Restores the admin from a recovery kit, into a new folder of its own.
    pub async fn restore(
        &self,
        recovery_kit: &str,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let workspace_id = kit_workspace_id(recovery_kit)?;
        let data = self.prepare_folder(&workspace_id)?;
        let workspace = Workspace::restore_with(
            &data,
            recovery_kit,
            self.connect.clone(),
            self.with_device_settings(options),
        )
        .await?;
        self.add_open(workspace)
    }

    /// Deletes a workspace from this device for good, closing it first: its
    /// folder, then its device key (a key left by a crash in between opens
    /// nothing). The next most recently used is then the selected one. If
    /// the folder is still in use elsewhere, leaving fails and the workspace
    /// stays here, closed; `open_all` opens it again. Once deleting starts,
    /// it is recorded, so a leave cut short (a file held by another program,
    /// a crash) finishes later (`finish_leaving`).
    pub async fn leave(&self, workspace_id: &str) -> Result<()> {
        self.leave_until(workspace_id, LeaveStep::Done).await
    }

    pub(crate) async fn leave_until(&self, workspace_id: &str, stop_after: LeaveStep) -> Result<()> {
        if self.settings()?.leaving.iter().any(|id| id == workspace_id) {
            return self.finish_leaving().await;
        }
        let data = self.folder(workspace_id)?;
        let stored_id = WorkspaceConfig::peek(&data.path)?.map(|(stored_id, _)| stored_id);
        ensure!(
            stored_id.as_deref() == Some(workspace_id),
            "no such workspace here"
        );
        let closing = {
            let mut open = self.open_lock();
            let index = open
                .iter()
                .position(|workspace| workspace.workspace_id() == workspace_id);
            index.map(|index| open.remove(index))
        };
        if let Some(workspace) = closing {
            // Leaving drops what wasn't uploaded anyway, so it doesn't wait long.
            match tokio::time::timeout(CLOSING_WAIT, workspace.shutdown()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!("leaving a workspace with changes not uploaded: {error:#}"),
                Err(_) => tracing::warn!("leaving a workspace whose upload didn't finish in time"),
            }
            workspace.stop_background();
        }
        self.pace();
        {
            // While the folder is locked, nothing can open it; without its
            // workspace.json, nothing will after.
            let _lock = self.wait_until_closed(&data).await?;
            self.update_settings(|settings| settings.leaving.push(workspace_id.to_owned()))?;
            let marker = data.path.join(crate::config::CONFIG_FILE);
            std::fs::remove_file(&marker).with_context(|| format!("deleting {}", marker.display()))?;
        }
        if stop_after == LeaveStep::MarkerDeleted {
            return Ok(());
        }
        self.finish_leaving().await
    }

    /// Finishes every leave that started deleting: the folder, then the
    /// key, then the record of it. One that fails again stays recorded.
    async fn finish_leaving(&self) -> Result<()> {
        let mut first_failure = None;
        for workspace_id in self.settings()?.leaving {
            let data = self.folder(&workspace_id)?;
            // In one folder, another workspace may have been made there since:
            // it isn't the one being left, so only the record goes.
            let is_someone_elses =
                WorkspaceConfig::peek(&data.path)?.is_some_and(|(stored_id, _)| stored_id != workspace_id);
            let finished = match is_someone_elses {
                true => Ok(()),
                false => self.delete_folder_and_key(&data).await,
            };
            match finished {
                Ok(()) => self.update_settings(|settings| {
                    for ids in [
                        &mut settings.leaving,
                        &mut settings.workspace_ids,
                        &mut settings.recent,
                        &mut settings.muted,
                    ] {
                        ids.retain(|id| *id != workspace_id);
                    }
                })?,
                Err(error) => {
                    first_failure.get_or_insert(error);
                }
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    /// The folder's lock, once a closing workspace's tasks let go of it.
    async fn wait_until_closed(&self, data: &DataDir) -> Result<std::fs::File> {
        let started = tokio::time::Instant::now();
        loop {
            match data.lock() {
                Ok(lock) => return Ok(lock),
                Err(error) if started.elapsed() >= CLOSING_WAIT => {
                    return Err(error.context("the workspace is still open; close it first"));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    /// Moves the one workspace folder a device had before there could be
    /// several (`old`) to its place here, once. Returns its id if it moved.
    /// Crash-safe: the device key is stored under the new folder's name
    /// first, then the folder is renamed, then the old key forgotten, and a
    /// move cut short finishes the next time.
    pub async fn adopt_single_folder(&self, old: &DataDir) -> Result<Option<String>> {
        self.move_single_folder(old, MoveStep::Done).await
    }

    pub(crate) async fn move_single_folder(
        &self,
        old: &DataDir,
        stop_after: MoveStep,
    ) -> Result<Option<String>> {
        if matches!(self.layout, Layout::OneFolder(_)) || self.settings()?.is_single_folder_handled {
            return Ok(None);
        }
        if !old.path.exists() {
            // Nothing to move, or a move that already renamed the folder and
            // may have left the old key. A keychain that can't be reached
            // now is tried again at the next start rather than stopping this one.
            let is_forgotten = match old.path.parent().is_some_and(Path::exists) {
                true => old.keys.forget(&old.path).await,
                false => Ok(()),
            };
            match is_forgotten {
                Ok(()) => self.update_settings(|settings| settings.is_single_folder_handled = true)?,
                Err(error) => tracing::warn!("couldn't forget an old device key: {error:#}"),
            }
            return Ok(None);
        }
        let Some((workspace_id, _)) = WorkspaceConfig::peek(&old.path)? else {
            // A join that never finished: nothing in it to keep.
            tracing::warn!(
                "{} isn't a complete workspace, so it isn't moved; it can be deleted",
                old.path.display()
            );
            self.update_settings(|settings| settings.is_single_folder_handled = true)?;
            return Ok(None);
        };
        let new = self.folder(&workspace_id)?;
        ensure!(
            !new.path.exists(),
            "can't move {}: {} already exists",
            old.path.display(),
            new.path.display()
        );
        std::fs::create_dir_all(self.layout.parent())?;
        let key = old.keys.load(&old.path).await?;
        match new.keys.load(&new.path).await {
            Ok(stored) if stored.to_bytes() == key.to_bytes() => {}
            Ok(_) => bail!("another device key is already stored for {}", new.path.display()),
            Err(_) => new.keys.save(&new.path, &key).await?,
        }
        if stop_after == MoveStep::KeyCopied {
            return Ok(None);
        }
        std::fs::rename(&old.path, &new.path)
            .with_context(|| format!("moving {} to {}", old.path.display(), new.path.display()))?;
        if stop_after == MoveStep::FolderMoved {
            return Ok(None);
        }
        old.keys.forget(&old.path).await?;
        self.place_last(&workspace_id)?;
        self.update_settings(|settings| settings.is_single_folder_handled = true)?;
        Ok(Some(workspace_id))
    }

    /// Deletes a folder (off the async threads: it may be large), then the
    /// device key it was locked with.
    async fn delete_folder_and_key(&self, data: &DataDir) -> Result<()> {
        let path = data.path.clone();
        let deleted = tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(anyhow::Error::new(error).context(format!("deleting {}", path.display()))),
        })
        .await?;
        deleted?;
        data.keys.forget(&data.path).await
    }

    /// A folder for a new workspace, refused if this device already has it,
    /// or is still deleting it.
    fn prepare_folder(&self, workspace_id: &str) -> Result<DataDir> {
        let data = self.folder(workspace_id)?;
        ensure!(
            !self.settings()?.leaving.iter().any(|id| id == workspace_id),
            "this workspace is still being deleted from this device; try again after restarting shyspace"
        );
        ensure!(
            !Workspace::exists(&data.path),
            "this workspace is already on this device"
        );
        std::fs::create_dir_all(self.layout.parent())?;
        Ok(data)
    }

    fn place_last(&self, workspace_id: &str) -> Result<()> {
        self.update_settings(|settings| {
            if !settings.workspace_ids.iter().any(|id| id == workspace_id) {
                settings.workspace_ids.push(workspace_id.to_owned());
            }
        })
    }

    /// Changes `workspaces.json` under the settings lock, so two changes
    /// can't interleave or lose one another.
    fn update_settings(&self, change: impl FnOnce(&mut DeviceSettings)) -> Result<()> {
        self.settings_file.update(change)
    }

    fn settings(&self) -> Result<DeviceSettings> {
        self.settings_file.read()
    }

    /// Opens every workspace here not open yet, the last used first, and
    /// selects the last used. One that fails to open doesn't stop the
    /// others.
    pub async fn open_all(&self, options: impl Fn() -> WorkspaceOptions) -> Result<Opened> {
        if let Err(error) = self.finish_leaving().await {
            tracing::error!("couldn't finish deleting a workspace that was left: {error:#}");
        }
        let recent = self.settings()?.recent;
        let mut ids: Vec<String> = self.list()?.into_iter().map(|entry| entry.workspace_id).collect();
        ids.sort_by_key(|id| {
            recent
                .iter()
                .position(|recent| recent == id)
                .unwrap_or(usize::MAX)
        });
        let mut opened = Opened::default();
        for workspace_id in ids {
            if self.find(&workspace_id).is_some() {
                continue;
            }
            let data = self.folder(&workspace_id)?;
            match Workspace::open_with(&data, self.connect.clone(), self.with_device_settings(options()))
                .await
            {
                Ok(workspace) => {
                    self.open_lock().push(workspace.clone());
                    opened.workspaces.push(workspace);
                }
                Err(error) => opened.failed.push((workspace_id, error)),
            }
        }
        self.pace();
        Ok(opened)
    }

    /// Takes every workspace out of the open ones, for the caller to shut
    /// down; `open_all` opens them again.
    pub fn close_all(&self) -> Vec<Workspace> {
        std::mem::take(&mut *self.open_lock())
    }

    /// The open workspace `workspace_id`, if it is open.
    pub fn find(&self, workspace_id: &str) -> Option<Workspace> {
        self.open_lock()
            .iter()
            .find(|workspace| workspace.workspace_id() == workspace_id)
            .cloned()
    }

    /// Every open workspace, the selected one first.
    pub fn open_workspaces(&self) -> Vec<Workspace> {
        self.open_lock().clone()
    }

    /// The folder joining with `invite_code` will use, so the joining
    /// device's safety code can be shown while the admin decides.
    pub fn folder_for_invite(&self, invite_code: &str) -> Result<DataDir> {
        self.folder(&Invite::decode(invite_code)?.workspace_id)
    }

    /// The selected workspace: the one the app shows.
    pub fn selected(&self) -> Option<Workspace> {
        self.open_lock().first().cloned()
    }

    /// Shows `workspace_id`: it becomes the most recently used, and so live.
    pub fn select(&self, workspace_id: &str) -> Result<Workspace> {
        let workspace = {
            let mut open = self.open_lock();
            let index = open
                .iter()
                .position(|workspace| workspace.workspace_id() == workspace_id)
                .context("that workspace isn't open")?;
            let workspace = open.remove(index);
            open.insert(0, workspace.clone());
            workspace
        };
        self.pace();
        self.update_settings(|settings| {
            settings.recent.retain(|id| id != workspace_id);
            settings.recent.insert(0, workspace_id.to_owned());
        })?;
        Ok(workspace)
    }

    /// The workspaces here as the rail shows them, in the person's order.
    pub fn rail(&self) -> Result<Vec<WorkspaceItem>> {
        let muted = self.settings()?.muted;
        let mut items = Vec::new();
        for entry in self.list()? {
            let open = self.find(&entry.workspace_id);
            // One unreadable icon shows a letter, not a broken rail.
            let icon = open.as_ref().and_then(|workspace| {
                workspace.workspace_icon_id().unwrap_or_else(|error| {
                    tracing::warn!("couldn't read a workspace's icon: {error:#}");
                    None
                })
            });
            items.push(WorkspaceItem {
                is_live: open.is_some_and(|workspace| workspace.cadence() == Cadence::Live),
                is_muted: muted.contains(&entry.workspace_id),
                icon,
                workspace_id: entry.workspace_id,
                name: entry.name,
            });
        }
        Ok(items)
    }

    pub fn mute(&self, workspace_id: &str) -> Result<()> {
        self.update_settings(|settings| {
            if !settings.muted.iter().any(|id| id == workspace_id) {
                settings.muted.push(workspace_id.to_owned());
            }
        })
    }

    pub fn unmute(&self, workspace_id: &str) -> Result<()> {
        self.update_settings(|settings| {
            settings.muted.retain(|id| id != workspace_id);
        })
    }

    /// How the app shows itself outside its window on this device.
    pub fn app_icon(&self) -> Result<AppIcon> {
        let settings = self.settings()?;
        Ok(AppIcon {
            is_in_menu_bar: settings.is_in_menu_bar.unwrap_or(false),
            shows_unread_count: settings.shows_unread_count.unwrap_or(true),
            is_tray_monochrome: settings.is_tray_monochrome.unwrap_or(false),
        })
    }

    pub fn set_in_menu_bar(&self, is_in_menu_bar: bool) -> Result<()> {
        self.update_settings(|settings| settings.is_in_menu_bar = Some(is_in_menu_bar))
    }

    pub fn set_shows_unread_count(&self, shows_unread_count: bool) -> Result<()> {
        self.update_settings(|settings| settings.shows_unread_count = Some(shows_unread_count))
    }

    pub fn set_tray_monochrome(&self, is_tray_monochrome: bool) -> Result<()> {
        self.update_settings(|settings| settings.is_tray_monochrome = Some(is_tray_monochrome))
    }

    /// Holds every workspace's notifications until `until_ms`.
    pub fn pause_notifications(&self, until_ms: i64) -> Result<()> {
        self.update_settings(|settings| {
            settings.notifications_paused_until_ms = Some(until_ms);
        })
    }

    pub fn resume_notifications(&self) -> Result<()> {
        self.update_settings(|settings| {
            settings.notifications_paused_until_ms = None;
        })
    }

    /// Until when notifications are paused, if they are now.
    pub fn notifications_paused(&self) -> Result<Option<i64>> {
        let now = now_ms()?;
        Ok(self
            .settings()?
            .notifications_paused_until_ms
            .filter(|until_ms| *until_ms > now))
    }

    /// Whether a notification from `workspace_id` may show now.
    pub fn may_notify(&self, workspace_id: &str) -> Result<bool> {
        let is_muted = self.settings()?.muted.iter().any(|id| id == workspace_id);
        Ok(!is_muted && self.notifications_paused()?.is_none())
    }

    /// The open workspace holding this device's call: there is one at most.
    pub fn in_huddle(&self) -> Option<Workspace> {
        self.open_workspaces()
            .into_iter()
            .find(|workspace| workspace.my_huddle().is_some())
    }

    /// This device's call, wherever it is, with where.
    pub fn my_huddle(&self) -> Result<Option<WorkspaceHuddle>> {
        let Some(workspace) = self.in_huddle() else {
            return Ok(None);
        };
        let Some(huddle) = workspace.my_huddle() else {
            return Ok(None);
        };
        Ok(Some(WorkspaceHuddle {
            workspace_id: workspace.workspace_id().to_owned(),
            channel_name: workspace.place_name(&huddle.channel)?,
            huddle,
        }))
    }

    /// A call ringing in any workspace.
    pub fn ringing(&self) -> Option<WorkspaceRing> {
        self.open_workspaces().into_iter().find_map(|workspace| {
            workspace.huddle_ringing().map(|ring| WorkspaceRing {
                workspace_id: workspace.workspace_id().to_owned(),
                ring,
            })
        })
    }

    /// Joins a call in `workspace_id`'s workspace, leaving any call in the
    /// others first: there is one microphone, so one call at a time.
    pub fn join_huddle(&self, workspace_id: &str, channel: &str, muted: bool) -> Result<()> {
        let workspace = self.find(workspace_id).context("that workspace isn't open")?;
        for other in self.open_workspaces() {
            if other.workspace_id() != workspace_id && other.my_huddle().is_some() {
                other.leave_huddle()?;
            }
        }
        workspace.join_huddle(channel, muted)
    }

    /// Away (or back) is about the person, so it shows in every workspace.
    pub fn set_away(&self, is_away: bool) {
        for workspace in self.open_workspaces() {
            workspace.set_away(is_away);
        }
    }

    /// Reminders that came due in any workspace, each handed out once.
    pub fn take_due_reminders(&self) -> Result<Vec<DueReminder>> {
        let mut due = Vec::new();
        for workspace in self.open_workspaces() {
            for item in workspace.take_due_reminders()? {
                due.push(DueReminder {
                    workspace_id: workspace.workspace_id().to_owned(),
                    item,
                });
            }
        }
        Ok(due)
    }

    /// When the next reminder in any workspace comes due.
    pub fn next_reminder_ms(&self) -> Result<Option<i64>> {
        let mut next = None;
        for workspace in self.open_workspaces() {
            next = match (next, workspace.next_reminder_ms()?) {
                (Some(earliest), Some(candidate)) => Some(std::cmp::min(earliest, candidate)),
                (earliest, candidate) => earliest.or(candidate),
            };
        }
        Ok(next)
    }

    /// The most recently used run live, the rest in the background.
    fn pace(&self) {
        for (index, workspace) in self.open_lock().iter().enumerate() {
            let cadence = match index < LIVE_WORKSPACES {
                true => Cadence::Live,
                false => Cadence::Background,
            };
            workspace.set_cadence(cadence);
        }
    }

    /// Opens a workspace just made here and selects it.
    fn add_open(&self, workspace: Workspace) -> Result<Workspace> {
        let workspace_id = workspace.workspace_id().to_owned();
        self.place_last(&workspace_id)?;
        self.open_lock().push(workspace);
        self.select(&workspace_id)
    }

    fn open_lock(&self) -> std::sync::MutexGuard<'_, Vec<Workspace>> {
        self.open
            .lock()
            .expect("open workspaces lock poisoned by an earlier panic")
    }
}

/// How far a leave gets before stopping, to test a crash part way.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaveStep {
    MarkerDeleted,
    Done,
}

/// How far a folder move gets before stopping, to test a crash part way.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveStep {
    KeyCopied,
    FolderMoved,
    Done,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_are_workspace_ids() {
        for id in ["3f2a9c", "workspace-id", "a_b", "console", "com10", "nul-1"] {
            assert!(is_plain_id(id), "{id}");
        }
        let device_names = ["con", "prn", "aux", "nul", "com0", "com9", "lpt0", "lpt9"];
        for id in ["", "../x", "a/b", "..", "a b", &"x".repeat(65)]
            .into_iter()
            .chain(device_names)
        {
            assert!(!is_plain_id(id), "{id}");
        }
    }
}
