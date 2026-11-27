//! Linking this device to a person who already uses shyspace, from the new
//! device's side: it sends its request through the code, shows the check
//! code, reads at once from what the person's device hands it, and joins
//! once the admin's app adds it, keeping what was typed meanwhile. What it
//! needs is kept beside its folder, so a restart picks the link up again.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use tokio::time::sleep;
use tracing::warn;
use zeroize::Zeroizing;

use super::{
    DataDir, DeviceEnrollment, DeviceLink, JoinCancel, Workspace, WorkspaceOptions, linking_store::KeptLink,
};
use crate::{
    membership::{
        admin::DeviceWelcome,
        handover::{Answer, Handover, open_handover},
        invite::JoinMailbox,
        link::{
            ANSWER_LIFETIME_MS, CheckReading, LINK_CODE_LIFETIME_MS, LinkCode, LinkRequest, Platform,
            read_check, seal_link_request,
        },
        link_answers::{Reason, SignedOutcome, SignedWelcome},
    },
    ops::now_ms,
    transport::{
        bucket::{Connector, read_only::ReadingConnector, s3_connector},
        presigned::PresignedMailbox,
    },
};

const POLL: Duration = Duration::from_secs(2);
const ADDED_POLL: Duration = Duration::from_secs(5);
const DID_NOT_FINISH: &str = "Linking didn't finish on this device. Link again from your other device.";
const NOT_ADDED_IN_TIME: &str =
    "This device wasn't added within a week, so nothing was kept. Link again from your other device.";

/// What this device shows once the person's device opened its request.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckOnNewDevice {
    /// The number the person taps on their other device.
    Code(String),
    /// The person's device answered another device: someone else used the
    /// code. Nothing was handed to this one.
    SomeoneElse,
    /// The person confirmed before this device showed the number.
    Confirmed,
}

/// The admin's app refused the link, with its signed reason. Both stores
/// are wiped by then.
#[derive(Debug)]
pub struct LinkRefused(pub Reason);

impl std::fmt::Display for LinkRefused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.0 {
            Reason::Cancelled => "Your other device stopped the link. Nothing was shared.",
            Reason::TooManyDevices => {
                "You already have 5 devices. Remove one on your other device, then link again."
            }
            Reason::DailyLimit => "You've linked several devices today. Try again tomorrow.",
            Reason::Stale | Reason::Refused => {
                "The workspace couldn't add this device. Link again from your other device."
            }
        })
    }
}

impl std::error::Error for LinkRefused {}

/// A link picked up after a restart.
pub enum ResumedLink<M: JoinMailbox> {
    /// Still waiting for the person's device to hand this one what it needs.
    Waiting(DeviceLinking<M>),
    /// Reading, and waiting to be added.
    Reading(DeviceLinking<M>, Workspace),
}

/// A link in progress on the new device.
pub struct DeviceLinking<M: JoinMailbox> {
    enrollment: DeviceEnrollment,
    linking: DataDir,
    code: LinkCode,
    request: LinkRequest,
    /// Gone once the hand-over opened.
    handover_secret: Option<Zeroizing<Vec<u8>>>,
    started_ms: i64,
    mailbox: M,
}

impl DeviceLinking<PresignedMailbox> {
    /// Starts linking this device, in the empty folder `data`, with the
    /// code the person's other device shows.
    pub async fn begin(data: DataDir, code: &str, platform: Platform) -> Result<Self> {
        let code = LinkCode::decode(code)?;
        let mailbox = PresignedMailbox::for_link(&code);
        Self::begin_with(data, code, platform, mailbox).await
    }

    /// Picks up a link into `data` after a restart: `None` if there is none.
    /// A link that began joining, or waited a week, is wiped with an error
    /// saying to link again.
    pub async fn resume(data: DataDir) -> Result<Option<ResumedLink<PresignedMailbox>>> {
        Self::resume_with(
            data,
            PresignedMailbox::for_link,
            ReadingConnector::over(s3_connector()),
        )
        .await
    }

    /// Waits until the person's device hands this one what it needs, then
    /// opens the workspace for reading.
    pub async fn wait_for_handover(&mut self, cancel: &JoinCancel) -> Result<Workspace> {
        self.wait_for_handover_with(cancel, ReadingConnector::over(s3_connector()))
            .await
    }

    /// Waits until the admin's app adds this device, then joins, keeping
    /// the drafts and read positions `reading` holds.
    pub async fn wait_until_added(
        self,
        reading: Workspace,
        cancel: &JoinCancel,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        self.wait_until_added_with(reading, cancel, s3_connector(), options)
            .await
    }
}

impl<M: JoinMailbox> DeviceLinking<M> {
    pub(crate) async fn begin_with(
        data: DataDir,
        code: LinkCode,
        platform: Platform,
        mailbox: M,
    ) -> Result<Self> {
        ensure!(
            !code.is_expired(now_ms()?),
            "This code has run out. Show a new one on your other device."
        );
        let linking = data.linking();
        let (enrollment, device_request) = DeviceEnrollment::begin(data, link_of(&code)).await?;
        let (request, handover_key) = enrollment.link_request(&code, &device_request, platform)?;
        linking
            .keep_link(enrollment.identity(), &code, &request, &handover_key.secret)
            .await?;
        mailbox.post_request(seal_link_request(&code, &request)?).await?;
        Ok(Self {
            enrollment,
            linking,
            code,
            request,
            handover_secret: Some(handover_key.secret),
            started_ms: now_ms()?,
            mailbox,
        })
    }

    pub(crate) async fn resume_with(
        data: DataDir,
        mailbox_for: impl FnOnce(&LinkCode) -> M,
        connect: ReadingConnector,
    ) -> Result<Option<ResumedLink<M>>> {
        let linking = data.linking();
        let Some(kept) = linking.kept_link().await? else {
            return Ok(None);
        };
        if Workspace::exists(&data.path) {
            // Joined before the app stopped: only the reading store was left.
            linking.forget_reading().await?;
            return Ok(None);
        }
        if kept.is_finishing || kept.is_expired(now_ms()?) {
            linking.forget_reading().await?;
            data.forget_unfinished().await?;
            bail!(match kept.is_finishing {
                true => DID_NOT_FINISH,
                false => NOT_ADDED_IN_TIME,
            });
        }
        let KeptLink {
            code,
            request,
            handover_secret,
            started_ms,
            ..
        } = kept;
        let enrollment = DeviceEnrollment::reopen(data, link_of(&code)).await?;
        let is_handed_over = handover_secret.is_none();
        let linking_handle = Self {
            enrollment,
            linking,
            mailbox: mailbox_for(&code),
            code,
            request,
            handover_secret,
            started_ms,
        };
        if !is_handed_over {
            return Ok(Some(ResumedLink::Waiting(linking_handle)));
        }
        let reading = Workspace::open_reading(&linking_handle.linking, connect).await?;
        Ok(Some(ResumedLink::Reading(linking_handle, reading)))
    }

    /// Makes this link look `age_ms` old.
    #[cfg(test)]
    pub(crate) fn backdate(&mut self, age_ms: i64) {
        self.started_ms -= age_ms;
    }

    /// Waits until the person's device opens this device's request.
    pub async fn wait_for_check(&self, cancel: &JoinCancel) -> Result<CheckOnNewDevice> {
        let mut cancelled = cancel.subscribe();
        loop {
            if let Some(check) = self.check_now().await? {
                return Ok(check);
            }
            ensure!(
                !self.code.is_expired(now_ms()?),
                "This code has run out. Show a new one on your other device."
            );
            tokio::select! {
                () = sleep(POLL) => {}
                _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you stopped linking this device"),
            }
        }
    }

    /// The check, if the person's device has answered. Junk in the answer
    /// slot, from someone with the bucket keys, is ignored.
    pub(crate) async fn check_now(&self) -> Result<Option<CheckOnNewDevice>> {
        let device = &self.request.device;
        match self.answer_now().await? {
            Some(Answer::Check(check)) => {
                match read_check(&self.code, device, &self.request.handover_key, &check) {
                    Ok(CheckReading::Code(code)) => Ok(Some(CheckOnNewDevice::Code(code))),
                    Ok(CheckReading::SomeoneElse) => Ok(Some(CheckOnNewDevice::SomeoneElse)),
                    Err(error) => {
                        warn!("ignoring a check the person's device didn't make: {error:#}");
                        Ok(None)
                    }
                }
            }
            Some(Answer::Handover(_)) => Ok(Some(CheckOnNewDevice::Confirmed)),
            None => Ok(None),
        }
    }

    async fn answer_now(&self) -> Result<Option<Answer>> {
        let Some(bytes) = self.mailbox.fetch_answer().await? else {
            return Ok(None);
        };
        Ok(Answer::decode(&bytes).ok())
    }

    pub(crate) async fn wait_for_handover_with(
        &mut self,
        cancel: &JoinCancel,
        connect: ReadingConnector,
    ) -> Result<Workspace> {
        let mut cancelled = cancel.subscribe();
        let answer_until_ms = self.code.expires_ms - LINK_CODE_LIFETIME_MS + ANSWER_LIFETIME_MS;
        loop {
            match self.handover_now().await {
                Ok(Some(handover)) => return self.open_reading_from(handover, connect).await,
                Ok(None) => {}
                Err(error) => warn!("ignoring a hand-over the person's device didn't make: {error:#}"),
            }
            ensure!(
                now_ms()? < answer_until_ms,
                "Your other device didn't finish the link in time. Link again."
            );
            tokio::select! {
                () = sleep(POLL) => {}
                _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you stopped linking this device"),
            }
        }
    }

    /// The hand-over, if the person's device has put it up.
    pub(crate) async fn handover_now(&self) -> Result<Option<Handover>> {
        let Some(Answer::Handover(sealed)) = self.answer_now().await? else {
            return Ok(None);
        };
        let secret = self
            .handover_secret
            .as_ref()
            .context("this device already opened its hand-over")?;
        open_handover(&self.code, &self.request.device, secret, &sealed).map(Some)
    }

    /// Opens the workspace for reading with `handover`, which spends the
    /// one-time secret.
    pub(crate) async fn open_reading_from(
        &mut self,
        handover: Handover,
        connect: ReadingConnector,
    ) -> Result<Workspace> {
        let reading =
            Workspace::open_reading_from_handover(&self.linking, &self.code, handover, connect).await?;
        self.handover_secret = None;
        Ok(reading)
    }

    pub(crate) async fn wait_until_added_with(
        self,
        reading: Workspace,
        cancel: &JoinCancel,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let mut cancelled = cancel.subscribe();
        loop {
            reading.sync().await;
            match self.added_now(&reading).await {
                Ok(Some(welcome)) => return self.finish(reading, welcome, connect, options).await,
                Ok(None) => {}
                Err(error) if error.downcast_ref::<LinkRefused>().is_some() => {
                    self.wipe(reading).await?;
                    return Err(error);
                }
                Err(error) => warn!("can't tell yet whether this device was added: {error:#}"),
            }
            if now_ms()? >= self.started_ms + super::linking_store::LINKING_LIFETIME_MS {
                self.wipe(reading).await?;
                bail!(NOT_ADDED_IN_TIME);
            }
            tokio::select! {
                () = sleep(ADDED_POLL) => {}
                _ = cancelled.wait_for(|is_cancelled| *is_cancelled) => bail!("you stopped linking this device"),
            }
        }
    }

    /// The admin's signed welcome, if there is one yet. A signed refusal is
    /// a [`LinkRefused`] error; anything not signed by the admin for this
    /// code and device is ignored.
    pub(crate) async fn added_now(&self, reading: &Workspace) -> Result<Option<DeviceWelcome>> {
        let (token, device, admin) = (&self.code.token, &self.request.device, &self.code.admin);
        let workspace_id = &self.code.workspace_id;
        let bucket = reading.bucket();
        if let Some(bytes) = bucket.link_reply(token, device, "welcome").await? {
            match SignedWelcome::decode(&bytes)
                .and_then(|signed| signed.read(admin, workspace_id, token, device))
            {
                Ok(welcome) => return Ok(Some(welcome)),
                Err(error) => warn!("ignoring a welcome the admin didn't sign: {error:#}"),
            }
        }
        if let Some(bytes) = bucket.link_reply(token, device, "outcome").await? {
            let reason = SignedOutcome::decode(&bytes)
                .and_then(|signed| Ok(signed.read(admin, workspace_id, token, device)?.reason));
            match reason {
                // The person's device vouches again.
                Ok(reason) if reason.is_retry() => {}
                Ok(reason) => return Err(LinkRefused(reason).into()),
                Err(error) => warn!("ignoring an outcome the admin didn't sign: {error:#}"),
            }
        }
        Ok(None)
    }

    /// Joins with `welcome` in the real folder, which starts with this
    /// device's drafts and read positions, then deletes the linking store.
    /// It is marked first: a join that stops partway can't run again.
    async fn finish(
        self,
        reading: Workspace,
        welcome: DeviceWelcome,
        connect: Connector,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let kept = reading
            .state()
            .store
            .personal_reading()
            .context("keeping what this device typed while it read")?;
        drop(reading);
        self.linking.mark_link_finishing().await?;
        self.enrollment.keep_personal_reading(&kept)?;
        let joined = self.enrollment.finish_with(welcome, connect, options).await;
        #[cfg(test)]
        if STOPS_BEFORE_FORGETTING_READING
            .lock()
            .expect("simulated stops lock poisoned")
            .contains(&self.linking.path)
        {
            return joined;
        }
        self.linking.forget_reading().await?;
        joined
    }

    /// Deletes both stores: the link is over and this device holds nothing.
    async fn wipe(self, reading: Workspace) -> Result<()> {
        drop(reading);
        self.linking.forget_reading().await?;
        self.enrollment.forget().await
    }
}

/// Linking stores whose link stops, as if the app quit, after the join and
/// before the reading store is deleted.
#[cfg(test)]
pub(crate) static STOPS_BEFORE_FORGETTING_READING: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

fn link_of(code: &LinkCode) -> DeviceLink {
    DeviceLink {
        workspace_id: code.workspace_id.clone(),
        workspace_name: code.workspace_name.clone(),
        admin: code.admin,
        admin_mls_key: code.admin_mls_key.clone(),
        person: code.person,
    }
}
