//! Linking a new device to a person who is already a member. The device
//! learns the workspace from one of the person's devices, makes its key and
//! agrees to belong to the person, that device vouches for it, the admin
//! adds it with [`Workspace::add_device`], and the device finishes joining
//! from the admin's [`DeviceWelcome`].

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;
use tracing::warn;

use super::{
    Workspace, WorkspaceOptions,
    lifecycle::{Accepted, Admitted, DataDir, JoinFacts, Local, admit, clear_spent_join, prepare_empty_dir},
};
use crate::{
    config::WorkspaceConfig,
    membership::{
        admin::DeviceWelcome,
        commit::{self, NewerCommit, VerifiedCommit},
        device,
        mls::{Mls, binding_message},
    },
    ops::now_ms,
    storage::DeviceSource,
    transport::bucket::{BucketConfig, Connector, s3_connector},
};

/// How many times the commit slot may hold something other than the
/// commit that added this device before the link ends, and how far apart
/// the reads are.
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
        // Nothing borrowed from the store is held across the wait, so the
        // future can move between threads.
        let checked =
            match admit(&local, &mut mls, &facts, accepted).and_then(|granted| Ok((granted, mls.epoch()?))) {
                Err(error) => Err(error),
                Ok((granted, join_epoch)) => {
                    let read = read_join_commit(&connect, &link, &granted.bucket, join_epoch).await;
                    read.and_then(|verified| {
                        record_own_certificate(&local, &mls, &verified, join_epoch, &facts, &link.person)
                    })
                    .map(|()| granted)
                }
            };
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

/// Checks the commit that added this device as every member does, and
/// records the device as `person`'s if its certificate counts there. The
/// row is kept like the grant's: its commit lies before this device's
/// join, so recomputing the map never counts it again.
fn record_own_certificate(
    local: &Local,
    mls: &Mls,
    verified: &VerifiedCommit,
    join_epoch: u64,
    facts: &JoinFacts<'_>,
    person: &PublicKey,
) -> Result<()> {
    let from_epoch = join_epoch.checked_sub(1).context("joined at the first epoch")?;
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

/// The admin's commit that moved the group from `from_epoch`. Waited for as
/// long as its slot is empty or the bucket can't be reached: the admin's
/// app puts it up at its next sync if the upload failed, and nothing has
/// been announced meanwhile. A slot that keeps holding something else ends
/// the link; one from a newer version asks for an update at once.
async fn read_join_commit(
    connect: &Connector,
    link: &DeviceLink,
    bucket_config: &BucketConfig,
    join_epoch: u64,
) -> Result<VerifiedCommit> {
    let from_epoch = join_epoch.checked_sub(1).context("joined at the first epoch")?;
    let bucket = connect(&link.workspace_id, bucket_config)?;
    let mut wrong_reads = 0;
    loop {
        let bytes = match bucket.commit(from_epoch).await {
            Ok(bytes) => bytes,
            Err(error) => {
                warn!("can't read the commit that added this device yet: {error:#}");
                None
            }
        };
        if let Some(bytes) = bytes {
            match commit::verify(&bytes, from_epoch, &link.admin) {
                Ok(verified) => return Ok(verified),
                Err(error) if error.downcast_ref::<NewerCommit>().is_some() => return Err(error),
                Err(error) => {
                    wrong_reads += 1;
                    if wrong_reads >= JOIN_COMMIT_READS {
                        return Err(error.context("couldn't read the commit that added this device"));
                    }
                }
            }
        }
        tokio::time::sleep(JOIN_COMMIT_RETRY).await;
    }
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
