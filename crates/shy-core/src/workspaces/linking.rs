//! Linking this device to a person who already uses shyspace, as the app's
//! set of workspaces runs it: one link at a time, into the folder of the
//! code's workspace. Its reading workspace is open and shown like any other
//! until the admin's app adds the device, then the real one takes its place.

use anyhow::{Result, anyhow, bail, ensure};

use super::Workspaces;
use crate::{
    membership::{invite::JoinMailbox, link::LinkCode, link::Platform},
    ops::now_ms,
    transport::{bucket::read_only::ReadingConnector, presigned::PresignedMailbox},
    views::LinkCodeSummary,
    workspace::{
        CheckOnNewDevice, DeviceLinking, JoinCancel, ResumedLink, Workspace, WorkspaceOptions,
        linking_store::LINKING_SUFFIX,
    },
};

const NOT_A_LINK_CODE: &str = "This isn't a link code. Copy it again from your other device.";
const CODE_RAN_OUT: &str = "This code has run out. Show a new one on your other device.";

/// Where this device's link stands, between the app's steps.
#[derive(Default)]
pub(super) enum LinkSlot {
    #[default]
    Idle,
    /// A step is running with the link.
    Busy,
    Held(Box<HeldLink>),
}

/// A link between steps, with its reading workspace once it has one.
pub(super) struct HeldLink {
    linking: DeviceLinking<LinkMailbox>,
    reading: Option<Workspace>,
}

fn held(linking: DeviceLinking<LinkMailbox>, reading: Option<Workspace>) -> LinkSlot {
    LinkSlot::Held(Box::new(HeldLink { linking, reading }))
}

/// What a restart found: a link still waiting for its hand-over, or one
/// reading, whose workspace is open and selected.
pub enum ResumedLinkStep {
    Waiting,
    Reading(Workspace),
}

/// How a new device reaches its code's slots: the code's presigned links,
/// or the in-memory bucket in tests.
pub(crate) enum LinkMailbox {
    Presigned(PresignedMailbox),
    #[cfg(test)]
    Bucket(crate::tests::support::BucketLinkMailbox),
}

impl JoinMailbox for LinkMailbox {
    async fn post_request(&self, request: Vec<u8>) -> Result<()> {
        match self {
            Self::Presigned(mailbox) => mailbox.post_request(request).await,
            #[cfg(test)]
            Self::Bucket(mailbox) => mailbox.post_request(request).await,
        }
    }

    async fn fetch_answer(&self) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Presigned(mailbox) => mailbox.fetch_answer().await,
            #[cfg(test)]
            Self::Bucket(mailbox) => mailbox.fetch_answer().await,
        }
    }
}

impl Workspaces {
    /// Whether `text` is a whole, well-formed code that hasn't run out, so a
    /// screen can take a pasted code at once; a plain reason if not. Reads
    /// nothing but the text.
    pub fn read_link_code(text: &str) -> Result<LinkCodeSummary> {
        let code = LinkCode::decode(text).map_err(|_| anyhow!(NOT_A_LINK_CODE))?;
        ensure!(!code.is_expired(now_ms()?), CODE_RAN_OUT);
        Ok(LinkCodeSummary {
            workspace_name: code.workspace_name,
        })
    }

    /// Starts linking this device with the code the person's other device
    /// shows, and waits until that device opens the request: the number to
    /// show, or `Confirmed` if the person was quicker. Someone else having
    /// used the code ends the link here.
    pub async fn begin_link(
        &self,
        code: &str,
        platform: Platform,
        cancel: &JoinCancel,
    ) -> Result<CheckOnNewDevice> {
        let decoded = LinkCode::decode(code)?;
        let data = self.prepare_folder(&decoded.workspace_id)?;
        self.take_idle_slot()?;
        let begun =
            DeviceLinking::begin_with(data, decoded.clone(), platform, self.link_mailbox(&decoded)).await;
        let mut linking = match begun {
            Ok(linking) => linking,
            Err(error) => {
                self.set_link_slot(LinkSlot::Idle);
                return Err(error);
            }
        };
        // Every other ending (someone else used the code, it ran out, a
        // cancel) wipes the link before it returns.
        match linking.wait_for_check(cancel).await {
            Ok(check) => {
                self.set_link_slot(held(linking, None));
                Ok(check)
            }
            Err(error) => {
                self.set_link_slot(LinkSlot::Idle);
                Err(error)
            }
        }
    }

    /// Waits until the person's device hands this one what it needs, then
    /// opens the workspace for reading and selects it.
    pub async fn wait_for_link_handover(&self, cancel: &JoinCancel) -> Result<Workspace> {
        let (mut linking, reading) = self.take_held_link()?;
        if let Some(reading) = reading {
            self.set_link_slot(held(linking, Some(reading)));
            bail!("this device is reading already: wait until it is added");
        }
        match linking
            .wait_for_handover_with(cancel, ReadingConnector::over(self.connect.clone()))
            .await
        {
            Ok(reading) => {
                self.set_link_slot(held(linking, Some(reading.clone())));
                self.add_open(reading)
            }
            Err(error) => {
                self.set_link_slot(LinkSlot::Idle);
                Err(error)
            }
        }
    }

    /// Waits until the admin's app adds this device, then joins and shows
    /// the real workspace in the reading one's place. A link that ends
    /// another way closes the reading workspace.
    pub async fn wait_until_linked(
        &self,
        cancel: &JoinCancel,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let (linking, reading) = self.take_held_link()?;
        let Some(reading) = reading else {
            self.set_link_slot(held(linking, None));
            bail!("this device isn't reading yet: wait for the hand-over first");
        };
        let workspace_id = reading.workspace_id().to_owned();
        let joined = linking
            .wait_until_added_with(
                reading,
                cancel,
                self.connect.clone(),
                self.with_device_settings(options),
            )
            .await;
        self.set_link_slot(LinkSlot::Idle);
        // The reading workspace goes from the open ones; its store goes
        // once the last handle to it closes.
        self.open_lock()
            .retain(|workspace| !(workspace.workspace_id() == workspace_id && workspace.is_reading_only()));
        let joined = joined?;
        self.open_lock().push(joined);
        self.select(&workspace_id)
    }

    /// Stops linking this device, for good, between steps: both stores are
    /// wiped. While a step runs, its `JoinCancel` stops it instead, which
    /// wipes them too. Never for shutting the app down: quitting keeps the
    /// link, and `resume_link` picks it up at the next start.
    pub async fn cancel_link(&self) -> Result<()> {
        let (linking, reading) = self.take_held_link()?;
        self.set_link_slot(LinkSlot::Idle);
        if let Some(reading) = &reading {
            let workspace_id = reading.workspace_id().to_owned();
            self.open_lock().retain(|workspace| {
                !(workspace.workspace_id() == workspace_id && workspace.is_reading_only())
            });
        }
        linking.cancel(reading).await
    }

    /// Picks up a link a restart interrupted, if there is one; call it after
    /// `open_all`. `None` when there is nothing to pick up, or a link is
    /// already in progress here. One that began joining or waited a week is
    /// wiped, with the error to show.
    pub async fn resume_link(&self) -> Result<Option<ResumedLinkStep>> {
        if !matches!(*self.link_lock(), LinkSlot::Idle) {
            return Ok(None);
        }
        let Some(workspace_id) = self.linking_folder()? else {
            return Ok(None);
        };
        let data = self.folder(&workspace_id)?;
        self.take_idle_slot()?;
        let connect = ReadingConnector::over(self.connect.clone());
        let resumed = DeviceLinking::resume_with(data, |code| self.link_mailbox(code), connect).await;
        match resumed {
            Ok(Some(ResumedLink::Waiting(linking))) => {
                self.set_link_slot(held(linking, None));
                Ok(Some(ResumedLinkStep::Waiting))
            }
            Ok(Some(ResumedLink::Reading(linking, reading))) => {
                self.set_link_slot(held(linking, Some(reading.clone())));
                Ok(Some(ResumedLinkStep::Reading(self.add_open(reading)?)))
            }
            Ok(None) => {
                self.set_link_slot(LinkSlot::Idle);
                Ok(None)
            }
            Err(error) => {
                self.set_link_slot(LinkSlot::Idle);
                Err(error)
            }
        }
    }

    /// Links reach `bucket` directly, as the in-memory bucket can't
    /// presign.
    #[cfg(test)]
    pub(crate) fn set_test_link_bucket(&self, bucket: crate::transport::bucket::Bucket) {
        if let Ok(mut slot) = self.test_link_bucket.lock() {
            *slot = Some(bucket);
        }
    }

    /// The workspace a link in progress is for, from its linking folder.
    fn linking_folder(&self) -> Result<Option<String>> {
        let parent = self.layout.parent();
        if !parent.try_exists()? {
            return Ok(None);
        }
        for entry in std::fs::read_dir(&parent)? {
            let name = entry?.file_name();
            let Some(workspace_id) = name.to_str().and_then(|name| name.strip_suffix(LINKING_SUFFIX)) else {
                continue;
            };
            if super::is_plain_id(workspace_id) {
                return Ok(Some(workspace_id.to_owned()));
            }
        }
        Ok(None)
    }

    fn link_mailbox(&self, code: &LinkCode) -> LinkMailbox {
        #[cfg(test)]
        if let Some(bucket) = self
            .test_link_bucket
            .lock()
            .ok()
            .and_then(|bucket| bucket.clone())
        {
            return LinkMailbox::Bucket(crate::tests::support::BucketLinkMailbox {
                bucket,
                token: code.token,
            });
        }
        LinkMailbox::Presigned(PresignedMailbox::for_link(code))
    }

    fn take_idle_slot(&self) -> Result<()> {
        let mut slot = self.link_lock();
        ensure!(
            matches!(*slot, LinkSlot::Idle),
            "this device is already linking; finish or stop that first"
        );
        *slot = LinkSlot::Busy;
        Ok(())
    }

    fn take_held_link(&self) -> Result<(DeviceLinking<LinkMailbox>, Option<Workspace>)> {
        let mut slot = self.link_lock();
        match std::mem::replace(&mut *slot, LinkSlot::Busy) {
            LinkSlot::Held(held) => Ok((held.linking, held.reading)),
            other => {
                *slot = other;
                bail!("there's no link waiting for this step on this device")
            }
        }
    }

    fn set_link_slot(&self, next: LinkSlot) {
        *self.link_lock() = next;
    }

    fn link_lock(&self) -> std::sync::MutexGuard<'_, LinkSlot> {
        self.link.lock().expect("link lock poisoned by an earlier panic")
    }
}
