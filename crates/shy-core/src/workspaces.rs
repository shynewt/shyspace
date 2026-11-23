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

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    BucketConfig, DataDir, JoinCancel, Profile, Workspace, WorkspaceOptions,
    config::WorkspaceConfig,
    keys::KeyStorage,
    membership::{invite::Invite, recovery::kit_workspace_id},
    ops::{new_random_id, now_ms},
    sync::Cadence,
    transport::bucket::{Connector, s3_connector},
    views::WorkspaceItem,
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
#[derive(Debug, Clone)]
pub enum DeviceKeys {
    /// The OS keychain, one entry per workspace folder.
    Keychain,
    /// One file per workspace in this folder, for servers without a
    /// keychain. Keep it outside the data folder, or it protects nothing.
    Files(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub workspace_id: String,
    pub name: String,
}

/// This device's choices about its workspaces.
#[derive(Default, Serialize, Deserialize)]
struct DeviceSettings {
    /// The order they're shown in.
    #[serde(default)]
    workspace_ids: Vec<String>,
    /// Most recently selected first.
    #[serde(default)]
    recent: Vec<String>,
    #[serde(default)]
    muted: Vec<String>,
    #[serde(default)]
    notifications_paused_until_ms: Option<i64>,
}

/// A workspace id names a folder, and comes from invites and recovery kits
/// someone else made, so only plain names are accepted, never a path.
fn is_plain_id(workspace_id: &str) -> bool {
    !workspace_id.is_empty()
        && workspace_id.len() <= MAX_ID_LENGTH
        && workspace_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub struct Workspaces {
    root: PathBuf,
    keys: DeviceKeys,
    connect: Connector,
    /// The open workspaces, the selected one first, then by last use.
    open: Mutex<Vec<Workspace>>,
}

impl Workspaces {
    /// The workspaces kept under `root` (the app's data folder).
    pub fn new(root: impl Into<PathBuf>, keys: DeviceKeys) -> Self {
        Self::with_connector(root, keys, s3_connector())
    }

    pub(crate) fn with_connector(root: impl Into<PathBuf>, keys: DeviceKeys, connect: Connector) -> Self {
        Self {
            root: root.into(),
            keys,
            connect,
            open: Mutex::default(),
        }
    }

    /// The folder and device key of `workspace_id`'s workspace.
    pub fn folder(&self, workspace_id: &str) -> Result<DataDir> {
        ensure!(is_plain_id(workspace_id), "{workspace_id:?} isn't a workspace id");
        let keys = match &self.keys {
            DeviceKeys::Keychain => KeyStorage::Keychain,
            DeviceKeys::Files(folder) => KeyStorage::File(folder.join(format!("{workspace_id}.key"))),
        };
        Ok(DataDir::new(self.root.join(FOLDERS).join(workspace_id), keys))
    }

    /// Every complete workspace here, in the order the person arranged them;
    /// ones not placed yet come last, by name.
    pub fn list(&self) -> Result<Vec<WorkspaceEntry>> {
        let folders = self.root.join(FOLDERS);
        let entries = match std::fs::read_dir(&folders) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).with_context(|| format!("reading {}", folders.display())),
        };
        let mut found = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let Some((workspace_id, name)) = WorkspaceConfig::peek(&path)? else {
                continue;
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
        let mut settings = self.settings()?;
        settings.workspace_ids = ids;
        self.save_settings(&settings)
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
            options,
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
        let workspace = Workspace::join_cancellable(&data, invite_code, profile, options, cancel).await?;
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
        let workspace = Workspace::restore_with(&data, recovery_kit, self.connect.clone(), options).await?;
        self.add_open(workspace)
    }

    /// Deletes a workspace from this device for good, closing it first: its
    /// folder, then its device key (a key left by a crash in between opens
    /// nothing). The next most recently used is then the selected one.
    pub async fn leave(&self, workspace_id: &str) -> Result<()> {
        let data = self.folder(workspace_id)?;
        ensure!(Workspace::exists(&data.path), "no such workspace here");
        let closing = {
            let mut open = self.open_lock();
            let index = open
                .iter()
                .position(|workspace| workspace.workspace_id() == workspace_id);
            index.map(|index| open.remove(index))
        };
        if let Some(workspace) = closing {
            // Changes not uploaded yet are dropped anyway by leaving.
            if let Err(error) = workspace.shutdown().await {
                tracing::warn!("leaving a workspace with changes not uploaded: {error:#}");
            }
        }
        drop(self.wait_until_closed(&data).await?);
        std::fs::remove_dir_all(&data.path).with_context(|| format!("deleting {}", data.path.display()))?;
        data.keys.forget(&data.path).await?;
        let mut settings = self.settings()?;
        for ids in [
            &mut settings.workspace_ids,
            &mut settings.recent,
            &mut settings.muted,
        ] {
            ids.retain(|id| id != workspace_id);
        }
        self.save_settings(&settings)?;
        self.pace();
        Ok(())
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
        if !old.path.exists() {
            // Nothing to move, or a move that already renamed the folder.
            if old.path.parent().is_some_and(Path::exists) {
                old.keys.forget(&old.path).await?;
            }
            return Ok(None);
        }
        let Some((workspace_id, _)) = WorkspaceConfig::peek(&old.path)? else {
            bail!(
                "{} isn't a complete workspace; move or delete it by hand",
                old.path.display()
            );
        };
        let new = self.folder(&workspace_id)?;
        ensure!(
            !new.path.exists(),
            "can't move {}: {} already exists",
            old.path.display(),
            new.path.display()
        );
        std::fs::create_dir_all(self.root.join(FOLDERS))?;
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
        Ok(Some(workspace_id))
    }

    /// A folder for a new workspace, refused if this device already has it.
    fn prepare_folder(&self, workspace_id: &str) -> Result<DataDir> {
        let data = self.folder(workspace_id)?;
        ensure!(
            !Workspace::exists(&data.path),
            "this workspace is already on this device"
        );
        std::fs::create_dir_all(self.root.join(FOLDERS))?;
        Ok(data)
    }

    fn place_last(&self, workspace_id: &str) -> Result<()> {
        let mut settings = self.settings()?;
        if !settings.workspace_ids.iter().any(|id| id == workspace_id) {
            settings.workspace_ids.push(workspace_id.to_owned());
        }
        self.save_settings(&settings)
    }

    fn settings(&self) -> Result<DeviceSettings> {
        let path = self.root.join(SETTINGS_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).with_context(|| format!("reading {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DeviceSettings::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Written whole to a new file and renamed over the old one, so a crash
    /// leaves either version, never half of one.
    fn save_settings(&self, settings: &DeviceSettings) -> Result<()> {
        let path = self.root.join(SETTINGS_FILE);
        let partial = self.root.join(format!("{SETTINGS_FILE}.partial"));
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(&partial, serde_json::to_vec_pretty(settings)?)
            .with_context(|| format!("writing {}", partial.display()))?;
        std::fs::rename(&partial, &path).with_context(|| format!("writing {}", path.display()))
    }

    /// Opens every workspace here not open yet, the last used first, and
    /// selects the last used. One that fails to open doesn't stop the
    /// others; returns those that failed, with why.
    pub async fn open_all(
        &self,
        options: impl Fn() -> WorkspaceOptions,
    ) -> Result<Vec<(String, anyhow::Error)>> {
        let recent = self.settings()?.recent;
        let mut ids: Vec<String> = self.list()?.into_iter().map(|entry| entry.workspace_id).collect();
        ids.sort_by_key(|id| {
            recent
                .iter()
                .position(|recent| recent == id)
                .unwrap_or(usize::MAX)
        });
        let mut failed = Vec::new();
        for workspace_id in ids {
            if self.find(&workspace_id).is_some() {
                continue;
            }
            let data = self.folder(&workspace_id)?;
            match Workspace::open_with(&data, self.connect.clone(), options()).await {
                Ok(workspace) => self.open_lock().push(workspace),
                Err(error) => failed.push((workspace_id, error)),
            }
        }
        self.pace();
        Ok(failed)
    }

    /// The open workspace `workspace_id`, if it is open.
    pub fn find(&self, workspace_id: &str) -> Option<Workspace> {
        self.open_lock()
            .iter()
            .find(|workspace| workspace.workspace_id() == workspace_id)
            .cloned()
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
        let mut settings = self.settings()?;
        settings.recent.retain(|id| id != workspace_id);
        settings.recent.insert(0, workspace_id.to_owned());
        self.save_settings(&settings)?;
        Ok(workspace)
    }

    /// The workspaces here as the rail shows them, in the person's order.
    pub fn rail(&self) -> Result<Vec<WorkspaceItem>> {
        let muted = self.settings()?.muted;
        Ok(self
            .list()?
            .into_iter()
            .map(|entry| WorkspaceItem {
                is_live: self
                    .find(&entry.workspace_id)
                    .is_some_and(|workspace| workspace.cadence() == Cadence::Live),
                is_muted: muted.contains(&entry.workspace_id),
                icon: None,
                workspace_id: entry.workspace_id,
                name: entry.name,
            })
            .collect())
    }

    pub fn mute(&self, workspace_id: &str) -> Result<()> {
        let mut settings = self.settings()?;
        if !settings.muted.iter().any(|id| id == workspace_id) {
            settings.muted.push(workspace_id.to_owned());
        }
        self.save_settings(&settings)
    }

    pub fn unmute(&self, workspace_id: &str) -> Result<()> {
        let mut settings = self.settings()?;
        settings.muted.retain(|id| id != workspace_id);
        self.save_settings(&settings)
    }

    /// Holds every workspace's notifications until `until_ms`.
    pub fn pause_notifications(&self, until_ms: i64) -> Result<()> {
        let mut settings = self.settings()?;
        settings.notifications_paused_until_ms = Some(until_ms);
        self.save_settings(&settings)
    }

    pub fn resume_notifications(&self) -> Result<()> {
        let mut settings = self.settings()?;
        settings.notifications_paused_until_ms = None;
        self.save_settings(&settings)
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
        for id in ["3f2a9c", "workspace-id", "a_b"] {
            assert!(is_plain_id(id), "{id}");
        }
        for id in ["", "../x", "a/b", "..", "a b", &"x".repeat(65)] {
            assert!(!is_plain_id(id), "{id}");
        }
    }
}
