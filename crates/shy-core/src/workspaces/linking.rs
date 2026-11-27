//! Linking this device to a person who already uses shyspace, as the app's
//! set of workspaces runs it: one link at a time, into the folder of the
//! code's workspace. Its reading workspace is open and shown like any other
//! until the admin's app adds the device, then the real one takes its place.
//!
//! Each step runs in a task this set owns: a caller that stops waiting (a
//! screen closed, a phone's job cancelled) doesn't stop the step, a caller
//! that asks again waits for the same step, and `cancel_link` stops it.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail, ensure};
use tokio::sync::watch;

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
const STEP_RUNNING: &str = "This device's link is busy with another step. Wait for it, or stop the link.";
const NO_LINK_WAITING: &str = "there's no link waiting for this step on this device";

/// Where this device's link stands.
#[derive(Default)]
pub(super) enum LinkSlot {
    #[default]
    Idle,
    /// A step runs in a task this set owns.
    Running(RunningStep),
    /// Between steps, with its reading workspace once it has one.
    Held(Box<HeldLink>),
}

pub(super) struct HeldLink {
    linking: DeviceLinking<LinkMailbox>,
    reading: Option<Workspace>,
}

pub(super) struct RunningStep {
    step: Step,
    /// Stops the step, which then wipes what the link holds.
    cancel: JoinCancel,
    /// Set once the step ends, for every caller waiting on it.
    outcome: watch::Receiver<Option<StepOutcome>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Begin,
    Handover,
    Added,
}

#[derive(Clone)]
enum StepOutcome {
    Check(CheckOnNewDevice),
    Opened(Workspace),
    Failed(String),
}

fn held(linking: DeviceLinking<LinkMailbox>, reading: Option<Workspace>) -> LinkSlot {
    LinkSlot::Held(Box::new(HeldLink { linking, reading }))
}

/// What a restart found: a link still waiting for its hand-over, with the
/// number to show if the person's device opened the request already, or
/// one reading, whose workspace is open and selected.
pub enum ResumedLinkStep {
    Waiting { code: Option<String> },
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

/// What a caller does with a step, decided under the slot's lock.
enum Next {
    /// The step runs already: wait for it.
    Join(watch::Receiver<Option<StepOutcome>>),
    /// Start it. The slot is marked running already; `taken` is what it
    /// held, to hand to the step or put back.
    Start {
        taken: LinkSlot,
        cancel: JoinCancel,
        tell: watch::Sender<Option<StepOutcome>>,
        outcome: watch::Receiver<Option<StepOutcome>>,
    },
}

impl Workspaces {
    /// Whether `text` is a whole, well-formed code that hasn't run out, so a
    /// screen can take a pasted code at once; a plain reason if not. Reads
    /// nothing but the text. The workspace's name is the code's own claim,
    /// unchecked until the person's device answers.
    pub fn read_link_code(text: &str) -> Result<LinkCodeSummary> {
        let code = read_code(text)?;
        Ok(LinkCodeSummary {
            workspace_name: code.workspace_name,
        })
    }

    /// Starts linking this device with the code the person's other device
    /// shows, and waits until that device opens the request: the number to
    /// show, or `Confirmed` if the person was quicker. Every other ending
    /// (someone else used the code, it ran out, a stop) wipes the link.
    /// Refused while another link is in progress here, even one only on
    /// disk after a restart: one link at a time.
    pub async fn begin_link(self: &Arc<Self>, code: &str, platform: Platform) -> Result<CheckOnNewDevice> {
        let decoded = read_code(code)?;
        if let Some(workspace_id) = self.linking_folder()?
            && !matches!(*self.link_lock(), LinkSlot::Running(_))
        {
            bail!(
                "This device is already linking to a workspace ({workspace_id}). Pick that link up, or stop it, first."
            );
        }
        let outcome = match self.next_step(Step::Begin)? {
            Next::Join(outcome) => outcome,
            Next::Start {
                cancel,
                tell,
                outcome,
                ..
            } => {
                let data = match self.prepare_folder(&decoded.workspace_id) {
                    Ok(data) => data,
                    Err(error) => {
                        self.set_link_slot(LinkSlot::Idle);
                        return Err(error);
                    }
                };
                let mailbox = self.link_mailbox(&decoded);
                self.spawn_step(tell, async move {
                    let mut linking = match DeviceLinking::begin_with(data, decoded, platform, mailbox).await
                    {
                        Ok(linking) => linking,
                        Err(error) => return (LinkSlot::Idle, failed(&error)),
                    };
                    match linking.wait_for_check(&cancel).await {
                        Ok(check) => (held(linking, None), StepOutcome::Check(check)),
                        Err(error) => (LinkSlot::Idle, failed(&error)),
                    }
                });
                outcome
            }
        };
        match wait_for(outcome).await? {
            StepOutcome::Check(check) => Ok(check),
            other => Err(unexpected(other)),
        }
    }

    /// Waits until the person's device hands this one what it needs, then
    /// opens the workspace for reading and selects it.
    pub async fn wait_for_link_handover(self: &Arc<Self>) -> Result<Workspace> {
        let outcome = match self.next_step(Step::Handover)? {
            Next::Join(outcome) => outcome,
            Next::Start {
                taken,
                cancel,
                tell,
                outcome,
            } => {
                let LinkSlot::Held(held_link) = taken else {
                    self.set_link_slot(taken);
                    bail!(NO_LINK_WAITING);
                };
                let HeldLink { mut linking, reading } = *held_link;
                if let Some(reading) = reading {
                    self.set_link_slot(held(linking, Some(reading)));
                    bail!("this device is reading already: wait until it is added");
                }
                let connect = ReadingConnector::over(self.connect.clone());
                let workspaces = self.clone();
                self.spawn_step(tell, async move {
                    match linking.wait_for_handover_with(&cancel, connect).await {
                        Ok(reading) => match workspaces.add_open(reading.clone()) {
                            Ok(shown) => (held(linking, Some(reading)), StepOutcome::Opened(shown)),
                            Err(error) => (held(linking, Some(reading)), failed(&error)),
                        },
                        Err(error) => (LinkSlot::Idle, failed(&error)),
                    }
                });
                outcome
            }
        };
        match wait_for(outcome).await? {
            StepOutcome::Opened(reading) => Ok(reading),
            other => Err(unexpected(other)),
        }
    }

    /// Waits until the admin's app adds this device, then joins and shows
    /// the real workspace in the reading one's place: the swap happens
    /// under one lock, and only then are the reading workspace's other
    /// handles told ([`crate::views::Change::ReadingClosed`]), so a screen
    /// told to let go finds the joined one by id at once. A link that ends
    /// another way closes the reading workspace.
    pub async fn wait_until_linked(
        self: &Arc<Self>,
        options: impl Into<WorkspaceOptions>,
    ) -> Result<Workspace> {
        let options = self.with_device_settings(options);
        let outcome = match self.next_step(Step::Added)? {
            Next::Join(outcome) => outcome,
            Next::Start {
                taken,
                cancel,
                tell,
                outcome,
            } => {
                let LinkSlot::Held(held_link) = taken else {
                    self.set_link_slot(taken);
                    bail!(NO_LINK_WAITING);
                };
                let HeldLink { linking, reading } = *held_link;
                let Some(reading) = reading else {
                    self.set_link_slot(held(linking, None));
                    bail!("this device isn't reading yet: wait for the hand-over first");
                };
                let connect = self.connect.clone();
                let workspaces = self.clone();
                self.spawn_step(tell, async move {
                    let workspace_id = reading.workspace_id().to_owned();
                    let register = {
                        let workspaces = workspaces.clone();
                        let workspace_id = workspace_id.clone();
                        move |joined: Workspace| workspaces.swap_in_joined(&workspace_id, joined)
                    };
                    let joined = linking
                        .wait_until_added_registering(reading, &cancel, connect, options, register)
                        .await;
                    // Ended another way: the reading workspace goes from the
                    // open ones; its store goes once the last handle closes.
                    workspaces.close_reading_workspace(&workspace_id);
                    match joined {
                        Ok(joined) => (LinkSlot::Idle, StepOutcome::Opened(joined)),
                        Err(error) => (LinkSlot::Idle, failed(&error)),
                    }
                });
                outcome
            }
        };
        match wait_for(outcome).await? {
            StepOutcome::Opened(joined) => Ok(joined),
            other => Err(unexpected(other)),
        }
    }

    /// Stops linking this device, for good: a running step is stopped, and
    /// what the link holds is wiped, even a link only on disk after a
    /// restart. Never for shutting the app down: quitting keeps the link,
    /// and `resume_link` picks it up at the next start.
    pub async fn cancel_link(self: &Arc<Self>) -> Result<()> {
        let taken = std::mem::take(&mut *self.link_lock());
        match taken {
            LinkSlot::Running(running) => {
                running.cancel.cancel();
                let outcome = running.outcome.clone();
                self.set_link_slot(LinkSlot::Running(running));
                // The step wipes as it stops; whatever it says, it ended.
                let _ = wait_for(outcome).await;
                Ok(())
            }
            LinkSlot::Held(held_link) => {
                let HeldLink { linking, reading } = *held_link;
                if let Some(reading) = &reading {
                    self.close_reading_workspace(reading.workspace_id());
                }
                linking.cancel(reading).await
            }
            LinkSlot::Idle => match self.resume_link().await? {
                Some(_) => Box::pin(self.cancel_link()).await,
                None => bail!("there's no link to stop on this device"),
            },
        }
    }

    /// Picks up a link a restart interrupted, if there is one; call it after
    /// `open_all`. `None` when there is nothing to pick up, or a link is
    /// already in progress here. One that began joining or waited a week is
    /// wiped, with the error to show.
    pub async fn resume_link(self: &Arc<Self>) -> Result<Option<ResumedLinkStep>> {
        if !matches!(*self.link_lock(), LinkSlot::Idle) {
            return Ok(None);
        }
        let Some(workspace_id) = self.linking_folder()? else {
            return Ok(None);
        };
        let data = self.folder(&workspace_id)?;
        let connect = ReadingConnector::over(self.connect.clone());
        match DeviceLinking::resume_with(data, |code| self.link_mailbox(code), connect).await? {
            None => Ok(None),
            Some(ResumedLink::Waiting(linking)) => {
                let code = match linking.check_now().await {
                    Ok(Some(CheckOnNewDevice::Code(code))) => Some(code),
                    Ok(_) => None,
                    Err(error) => {
                        linking.cancel(None).await?;
                        return Err(error);
                    }
                };
                self.set_link_slot(held(linking, None));
                Ok(Some(ResumedLinkStep::Waiting { code }))
            }
            Some(ResumedLink::Reading(linking, reading)) => {
                self.set_link_slot(held(linking, Some(reading.clone())));
                Ok(Some(ResumedLinkStep::Reading(self.add_open(reading)?)))
            }
        }
    }

    /// Decides, under the slot's lock, whether `step` runs already (wait for
    /// it) or starts (the slot is marked running at once, so a second
    /// caller waits for the same step).
    fn next_step(&self, step: Step) -> Result<Next> {
        let mut slot = self.link_lock();
        match &*slot {
            LinkSlot::Running(running) if running.step == step => {
                return Ok(Next::Join(running.outcome.clone()));
            }
            LinkSlot::Running(_) => bail!(STEP_RUNNING),
            LinkSlot::Held(_) if step == Step::Begin => {
                bail!("this device is already linking; finish or stop that first")
            }
            LinkSlot::Idle if step != Step::Begin => bail!(NO_LINK_WAITING),
            _ => {}
        }
        let cancel = JoinCancel::new();
        let (tell, outcome) = watch::channel(None);
        let running = LinkSlot::Running(RunningStep {
            step,
            cancel: cancel.clone(),
            outcome: outcome.clone(),
        });
        Ok(Next::Start {
            taken: std::mem::replace(&mut *slot, running),
            cancel,
            tell,
            outcome,
        })
    }

    /// Runs a step in a task this set owns: dropping the caller's future
    /// doesn't stop it. It leaves the slot as the step ends, then tells
    /// every caller waiting on it.
    fn spawn_step(
        self: &Arc<Self>,
        tell: watch::Sender<Option<StepOutcome>>,
        step: impl std::future::Future<Output = (LinkSlot, StepOutcome)> + Send + 'static,
    ) {
        let workspaces = self.clone();
        tokio::spawn(async move {
            let (next, ended) = step.await;
            workspaces.set_link_slot(next);
            tell.send_replace(Some(ended));
        });
    }

    /// Puts the joined workspace in the reading one's place, under one lock,
    /// and selects it.
    fn swap_in_joined(&self, workspace_id: &str, joined: Workspace) -> Result<Workspace> {
        {
            let mut open = self.open_lock();
            open.retain(|workspace| {
                !(workspace.workspace_id() == workspace_id && workspace.is_reading_only())
            });
            open.push(joined);
        }
        self.select(workspace_id)
    }

    /// Takes the reading workspace for `workspace_id` out of the open ones.
    fn close_reading_workspace(&self, workspace_id: &str) {
        self.open_lock()
            .retain(|workspace| !(workspace.workspace_id() == workspace_id && workspace.is_reading_only()));
    }

    /// The workspace a link in progress is for: a linking folder holding a
    /// kept link. Empty or half-made leftovers are passed over.
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
            if !super::is_plain_id(workspace_id) {
                continue;
            }
            if self.folder(workspace_id)?.linking().store_path().try_exists()? {
                return Ok(Some(workspace_id.to_owned()));
            }
        }
        Ok(None)
    }

    /// Links reach `bucket` directly, as the in-memory bucket can't
    /// presign.
    #[cfg(test)]
    pub(crate) fn set_test_link_bucket(&self, bucket: crate::transport::bucket::Bucket) {
        if let Ok(mut slot) = self.test_link_bucket.lock() {
            *slot = Some(bucket);
        }
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

    fn set_link_slot(&self, next: LinkSlot) {
        *self.link_lock() = next;
    }

    fn link_lock(&self) -> std::sync::MutexGuard<'_, LinkSlot> {
        self.link.lock().expect("link lock poisoned by an earlier panic")
    }
}

/// A code as pasted, with the plain reason if it can't be used.
fn read_code(text: &str) -> Result<LinkCode> {
    let code = LinkCode::decode(text).map_err(|_| anyhow!(NOT_A_LINK_CODE))?;
    ensure!(!code.is_expired(now_ms()?), CODE_RAN_OUT);
    Ok(code)
}

fn failed(error: &anyhow::Error) -> StepOutcome {
    StepOutcome::Failed(format!("{error:#}"))
}

/// Waits until the step ends, holding nothing of the channel across it.
async fn wait_for(mut outcome: watch::Receiver<Option<StepOutcome>>) -> Result<StepOutcome> {
    let ended = outcome
        .wait_for(Option::is_some)
        .await
        .map_err(|_| anyhow!("the link step stopped unexpectedly"))?
        .clone();
    ended.context("the link step ended without saying how")
}

fn unexpected(outcome: StepOutcome) -> anyhow::Error {
    match outcome {
        StepOutcome::Failed(message) => anyhow!(message),
        _ => anyhow!("the link step ended in another state than asked"),
    }
}
