//! Linking a new device to a person who is already a member. The device
//! learns the workspace from one of the person's devices, makes its key and
//! agrees to belong to the person, that device vouches for it, the admin
//! adds it with [`Workspace::add_device`], and the device finishes joining
//! from the admin's [`DeviceWelcome`].

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;

use super::{
    Workspace, WorkspaceOptions,
    lifecycle::{Accepted, Admitted, DataDir, JoinFacts, Local, admit, clear_spent_join, prepare_empty_dir},
};
use crate::{
    config::WorkspaceConfig,
    membership::{
        admin::DeviceWelcome,
        commit::{self, VerifiedCommit},
        device,
        mls::{Mls, binding_message},
    },
    ops::now_ms,
    storage::DeviceSource,
    transport::bucket::{Bucket, Connector, s3_connector},
};

/// How often, and how far apart, the device tries to read the commit that
/// added it: the admin's app or a member may still be putting it up.
const JOIN_COMMIT_READS: u32 = 30;
#[cfg(not(test))]
const JOIN_COMMIT_RETRY: Duration = Duration::from_secs(2);
#[cfg(test)]
const JOIN_COMMIT_RETRY: Duration = Duration::from_millis(50);

/// What one of the person's devices tells the new device before it asks to
/// be linked. Who the admin is comes from here, never from the welcome.
#[derive(Debug, Clone)]
pub struct DeviceLink {
    pub workspace_id: String,
    pub workspace_name: String,
    pub admin: PublicKey,
    /// The admin's MLS signature key, so the device can tell the Welcome
    /// is the admin's.
    pub admin_mls_key: Vec<u8>,
    /// The person the device is to belong to.
    pub person: PublicKey,
}

/// A device on its way to being linked to a person.
pub struct DeviceEnrollment {
    data: DataDir,
    local: Local,
    mls: Mls,
    link: DeviceLink,
}

/// What the new device hands the person's device that vouches for it.
#[derive(Debug, Clone)]
pub struct DeviceRequest {
    pub device: PublicKey,
    pub key_package: Vec<u8>,
    /// The device's identity-key signature over its MLS signature key.
    pub binding: Vec<u8>,
    /// The device's signature agreeing to belong to the person.
    pub acceptance: Vec<u8>,
}

impl DeviceEnrollment {
    /// Makes this device's key in the empty folder `data`, to be linked as
    /// `link` says.
    pub async fn begin(data: DataDir, link: DeviceLink) -> Result<(Self, DeviceRequest)> {
        prepare_empty_dir(&data.path)?;
        let local = data.open_local().await?;
        let mls = local.open_mls(&data, None)?;
        let request = DeviceRequest {
            device: local.identity.public(),
            key_package: mls.key_package()?,
            binding: local
                .identity
                .sign(&binding_message(mls.signer_public()))
                .to_bytes()
                .to_vec(),
            acceptance: device::accept(&local.identity, &link.workspace_id, &link.person)?,
        };
        let enrollment = Self {
            data,
            local,
            mls,
            link,
        };
        Ok((enrollment, request))
    }

    /// Joins with the admin's welcome. Fails, and clears the folder so the
    /// next try makes a new key, if the device map doesn't match or the
    /// commit that added this device doesn't make it the person's.
    pub async fn finish(
        self,
        welcome: DeviceWelcome,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        self.finish_with(welcome, s3_connector(), options).await
    }

    pub(crate) async fn finish_with(
        self,
        welcome: DeviceWelcome,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let Self {
            data,
            local,
            mut mls,
            link,
        } = self;
        let facts = JoinFacts {
            workspace_id: &link.workspace_id,
            admin: link.admin,
            admin_mls_key: &link.admin_mls_key,
        };
        let accepted = Accepted {
            welcome: welcome.welcome,
            sealed_grant: welcome.sealed_grant,
            map_digest: Some(welcome.map_digest),
        };
        let checked = async {
            let granted = admit(&local, &mut mls, &facts, accepted)?;
            let bucket = connect(&link.workspace_id, &granted.bucket)?;
            record_own_certificate(&local, &mls, &bucket, &facts, &link.person).await?;
            anyhow::Ok(granted)
        }
        .await;
        // MLS deletes our key as it reads the Welcome, so a link that
        // fails from here on can't be finished from this folder.
        let granted = match checked {
            Ok(granted) => granted,
            Err(error) => {
                drop(mls);
                drop(local);
                clear_spent_join(&data).await;
                return Err(error);
            }
        };
        let admitted = Admitted {
            config: WorkspaceConfig {
                workspace_id: link.workspace_id,
                name: link.workspace_name,
                admin: link.admin,
                bucket: granted.bucket,
            },
            bucket_lamport: granted.bucket_lamport,
        };
        Workspace::complete_join(&data, local, mls, admitted, None, connect, options).await
    }
}

/// Reads the commit that added this device as every member does, and
/// records the device as `person`'s if its certificate counts there. The
/// row is kept like the grant's: its commit lies before this device's
/// join, so recomputing the map never counts it again.
async fn record_own_certificate(
    local: &Local,
    mls: &Mls,
    bucket: &Bucket,
    facts: &JoinFacts<'_>,
    person: &PublicKey,
) -> Result<()> {
    let join_epoch = mls.epoch()?;
    let from_epoch = join_epoch.checked_sub(1).context("joined at the first epoch")?;
    let verified = read_join_commit(bucket, from_epoch, &facts.admin).await?;
    let epoch_key = local
        .store
        .epoch_key(from_epoch)?
        .context("the grant has no key for the epoch before joining")?;
    let certificates = verified.certificates(from_epoch, &epoch_key)?;
    let expected = commit::lists_hash(&epoch_key, from_epoch, &verified.frozen, &certificates)?;
    ensure!(
        Mls::commit_binding(&verified.commit)? == expected,
        "the commit that added this device doesn't match its lists"
    );
    // The members after the commit come from the Welcome's tree, which MLS
    // checked; the grant must say the same.
    let mut members_after = mls.members()?;
    let mut granted_members = local.store.epoch_members(join_epoch)?;
    members_after.sort_unstable();
    granted_members.sort_unstable();
    ensure!(
        members_after == granted_members,
        "the grant's members don't match the group this device joined"
    );
    let me = local.identity.public();
    let mine = local
        .store
        .count_certificates(facts.workspace_id, from_epoch, &members_after, &certificates)?
        .into_iter()
        .find(|record| record.certificate.device == me && record.certificate.person == *person);
    let Some(record) = mine else {
        bail!(NotThePersons);
    };
    local.store.put_device(&record, DeviceSource::Grant, now_ms()?)
}

/// The admin's commit that moved the group from `from_epoch`, once it is in
/// the bucket and verifies; tried for a while first, as it may still be on
/// its way up. Nothing has been announced yet, so waiting costs nothing.
async fn read_join_commit(bucket: &Bucket, from_epoch: u64, admin: &PublicKey) -> Result<VerifiedCommit> {
    let mut last_error = anyhow::anyhow!("not tried");
    for _ in 0..JOIN_COMMIT_READS {
        let read = async {
            let bytes = bucket
                .commit(from_epoch)
                .await?
                .context("the commit that added this device isn't in the bucket")?;
            commit::verify(&bytes, from_epoch, admin)
        };
        match read.await {
            Ok(verified) => return Ok(verified),
            Err(error) => last_error = error,
        }
        tokio::time::sleep(JOIN_COMMIT_RETRY).await;
    }
    Err(last_error.context("couldn't read the commit that added this device"))
}

/// The commit that added this device doesn't make it the person's, so
/// every member takes it for a person of its own.
#[derive(Debug)]
struct NotThePersons;

impl std::fmt::Display for NotThePersons {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "the workspace didn't link this device to that person; link it again to make a new key",
        )
    }
}

impl std::error::Error for NotThePersons {}
