//! Every workspace on the device: listing, opening, creating.

use std::sync::Arc;

use shy_core::{
    BucketConfig, Networking, Profile,
    membership::link::Platform,
    views::{LinkCodeSummary, WorkspaceItem},
    workspace::CheckOnNewDevice,
    workspaces::{DeviceKeys, ResumedLinkStep, Workspaces},
};

use crate::{
    DeviceKeyStore, ShyError, ShyWorkspace,
    keys::AppKeyStore,
    runtime::{run, run_blocking},
};

/// The workspaces kept in the app's data folder.
#[derive(uniffi::Object)]
pub struct ShyApp {
    workspaces: Arc<Workspaces>,
    networking: Networking,
    /// One opening at a time: the core's would open a workspace twice if
    /// two ran together, as when screens restore at once.
    opening: Arc<tokio::sync::Mutex<()>>,
}

/// A link a restart interrupted: still waiting for the hand-over, or reading.
#[derive(uniffi::Enum)]
pub enum ResumedLink {
    /// `code` is the number to show again, if the other device had opened the request.
    Waiting {
        code: Option<String>,
    },
    Reading {
        workspace: Arc<ShyWorkspace>,
    },
}

/// Bucket settings, opaque so the secret key can't end up in a log line.
#[derive(uniffi::Object)]
pub struct BucketSettings(BucketConfig);

#[derive(Debug, uniffi::Record)]
pub struct OpenFailure {
    pub workspace_id: String,
    pub reason: String,
}

#[uniffi::export]
impl BucketSettings {
    #[uniffi::constructor]
    pub fn new(
        endpoint: String,
        region: String,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Arc<Self> {
        Arc::new(Self(BucketConfig {
            endpoint,
            region,
            bucket,
            access_key_id,
            secret_access_key,
        }))
    }
}

#[uniffi::export]
impl ShyApp {
    /// The workspaces in `data_dir`, with their device keys in `key_store`.
    #[uniffi::constructor]
    pub fn new(data_dir: String, key_store: Arc<dyn DeviceKeyStore>, networking: Networking) -> Arc<Self> {
        let keys = DeviceKeys::Host(Arc::new(AppKeyStore(key_store)));
        Arc::new(Self {
            workspaces: Arc::new(Workspaces::new(data_dir, keys)),
            networking,
            opening: Arc::default(),
        })
    }

    /// Opens every workspace not open yet. One that fails doesn't stop the
    /// others; the failures are returned. Safe to call from several places
    /// at once: they take turns.
    pub async fn open_all(&self) -> Result<Vec<OpenFailure>, ShyError> {
        let (workspaces, networking, opening) =
            (self.workspaces.clone(), self.networking, self.opening.clone());
        run(async move {
            let _turn = opening.lock().await;
            let opened = workspaces.open_all(|| networking.into()).await?;
            Ok(opened
                .failed
                .into_iter()
                .map(|(workspace_id, error)| OpenFailure {
                    workspace_id,
                    reason: format!("{error:#}"),
                })
                .collect())
        })
        .await
    }

    /// Whether `text` is a whole link code that hasn't run out, so a screen
    /// can take it at once; the core's words for the person if not.
    pub fn read_link_code(&self, text: String) -> Result<LinkCodeSummary, ShyError> {
        Ok(Workspaces::read_link_code(&text)?)
    }

    /// Starts linking this device with the code from the person's other
    /// device, and waits until that device opens the request. Like every
    /// link step, it runs on in the core if the caller stops waiting, and
    /// calling it again waits for that same run.
    pub async fn begin_link(&self, code: String, platform: Platform) -> Result<CheckOnNewDevice, ShyError> {
        let workspaces = self.workspaces.clone();
        run(async move { workspaces.begin_link(&code, platform).await }).await
    }

    /// Waits until the person's device hands this one what it needs; the
    /// workspace then opens for reading.
    pub async fn wait_for_link_handover(&self) -> Result<Arc<ShyWorkspace>, ShyError> {
        let workspaces = self.workspaces.clone();
        run(async move { Ok(ShyWorkspace::new(workspaces.wait_for_link_handover().await?)) }).await
    }

    /// Waits until the admin's app adds this device; the joined workspace
    /// then takes the reading one's place (`Change::ReadingClosed`).
    pub async fn wait_until_linked(&self) -> Result<Arc<ShyWorkspace>, ShyError> {
        let (workspaces, networking) = (self.workspaces.clone(), self.networking);
        run(async move { Ok(ShyWorkspace::new(workspaces.wait_until_linked(networking).await?)) }).await
    }

    /// Stops linking this device for good, a running step too. Not for
    /// closing the app: a link survives that and resumes.
    pub async fn cancel_link(&self) -> Result<(), ShyError> {
        let workspaces = self.workspaces.clone();
        run(async move { workspaces.cancel_link().await }).await
    }

    /// Picks up a link a restart interrupted, after `open_all`.
    pub async fn resume_link(&self) -> Result<Option<ResumedLink>, ShyError> {
        let workspaces = self.workspaces.clone();
        run(async move {
            Ok(workspaces.resume_link().await?.map(|step| match step {
                ResumedLinkStep::Waiting { code } => ResumedLink::Waiting { code },
                ResumedLinkStep::Reading(workspace) => ResumedLink::Reading {
                    workspace: ShyWorkspace::new(workspace),
                },
            }))
        })
        .await
    }

    /// The workspaces here, in the order the person arranged them.
    pub async fn workspaces(&self) -> Result<Vec<WorkspaceItem>, ShyError> {
        let workspaces = self.workspaces.clone();
        run_blocking(move || workspaces.rail()).await
    }

    /// Creates a workspace with this device's person as its admin.
    pub async fn create_workspace(
        &self,
        workspace_name: String,
        member_name: String,
        bucket: Arc<BucketSettings>,
    ) -> Result<Arc<ShyWorkspace>, ShyError> {
        let (workspaces, networking) = (self.workspaces.clone(), self.networking);
        run(async move {
            let profile = Profile {
                name: &member_name,
                is_bot: false,
            };
            let workspace = workspaces
                .create(&workspace_name, profile, bucket.0.clone(), networking)
                .await?;
            Ok(ShyWorkspace::new(workspace))
        })
        .await
    }

    /// An open workspace.
    pub fn workspace(&self, workspace_id: String) -> Option<Arc<ShyWorkspace>> {
        self.workspaces.find(&workspace_id).map(ShyWorkspace::new)
    }

    /// Closes every open workspace, after uploading what it can. One that fails
    /// doesn't stop the others; the failures are reported together.
    pub async fn close_all(&self) -> Result<(), ShyError> {
        let workspaces = self.workspaces.clone();
        run(async move {
            let mut failures = Vec::new();
            for workspace in workspaces.close_all() {
                let workspace_id = workspace.workspace_id().to_owned();
                if let Err(error) = workspace.shutdown().await {
                    failures.push(format!("{workspace_id}: {error:#}"));
                }
            }
            anyhow::ensure!(
                failures.is_empty(),
                "couldn't close every workspace: {}",
                failures.join("; ")
            );
            Ok(())
        })
        .await
    }
}

#[cfg(any(test, feature = "memory-bucket"))]
#[uniffi::export]
impl ShyApp {
    /// Like `new`, but every workspace uses one bucket kept in memory, gone
    /// when the app quits. For tests and demos.
    #[uniffi::constructor]
    pub fn with_memory_bucket(
        data_dir: String,
        key_store: Arc<dyn DeviceKeyStore>,
        networking: Networking,
    ) -> Arc<Self> {
        let keys = DeviceKeys::Host(Arc::new(AppKeyStore(key_store)));
        Arc::new(Self {
            workspaces: Arc::new(Workspaces::with_memory_bucket(data_dir, keys)),
            networking,
            opening: Arc::default(),
        })
    }
}
