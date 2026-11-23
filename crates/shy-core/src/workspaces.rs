//! The workspaces on this device. Each lives in its own folder,
//! `workspaces/<workspace id>/`, with its own device key, identity and
//! connections, so two of them share nothing and can't tell they share a
//! person; `workspaces.json` keeps only the order they're shown in. A
//! device from before there could be several has one folder, moved here
//! once (`adopt_single_folder`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    BucketConfig, DataDir, JoinCancel, Profile, Workspace, WorkspaceOptions,
    config::WorkspaceConfig,
    keys::KeyStorage,
    membership::{invite::Invite, recovery::kit_workspace_id},
    ops::new_random_id,
    transport::bucket::{Connector, s3_connector},
};

const FOLDERS: &str = "workspaces";
const ORDER_FILE: &str = "workspaces.json";
const MAX_ID_LENGTH: usize = 64;

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

#[derive(Default, Serialize, Deserialize)]
struct Order {
    workspace_ids: Vec<String>,
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
        let order = self.order()?.workspace_ids;
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
        self.save_order(&Order { workspace_ids: ids })
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
        self.place_last(&workspace_id)?;
        Ok(workspace)
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
        self.place_last(&workspace_id)?;
        Ok(workspace)
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
        self.place_last(&workspace_id)?;
        Ok(workspace)
    }

    /// Deletes a closed workspace from this device for good: its folder,
    /// then its device key (a key left by a crash in between opens nothing).
    pub async fn leave(&self, workspace_id: &str) -> Result<()> {
        let data = self.folder(workspace_id)?;
        ensure!(Workspace::exists(&data.path), "no such workspace here");
        drop(data.lock().context("close the workspace before leaving it")?);
        std::fs::remove_dir_all(&data.path).with_context(|| format!("deleting {}", data.path.display()))?;
        data.keys.forget(&data.path).await?;
        let mut order = self.order()?;
        order.workspace_ids.retain(|id| id != workspace_id);
        self.save_order(&order)
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
        let mut order = self.order()?;
        if !order.workspace_ids.iter().any(|id| id == workspace_id) {
            order.workspace_ids.push(workspace_id.to_owned());
        }
        self.save_order(&order)
    }

    fn order(&self) -> Result<Order> {
        let path = self.root.join(ORDER_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).with_context(|| format!("reading {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Order::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Written whole to a new file and renamed over the old one, so a crash
    /// leaves either order, never half of one.
    fn save_order(&self, order: &Order) -> Result<()> {
        let path = self.root.join(ORDER_FILE);
        let partial = self.root.join(format!("{ORDER_FILE}.partial"));
        std::fs::create_dir_all(&self.root)?;
        std::fs::write(&partial, serde_json::to_vec_pretty(order)?)
            .with_context(|| format!("writing {}", partial.display()))?;
        std::fs::rename(&partial, &path).with_context(|| format!("writing {}", path.display()))
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
