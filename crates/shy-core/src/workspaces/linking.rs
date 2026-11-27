//! Linking this device to a person who already uses shyspace, as the app's
//! set of workspaces runs it: one link at a time, into the folder of the
//! code's workspace. Its reading workspace is open and shown like any other
//! until the admin's app adds the device, then the real one takes its place.
//!
//! Each step runs in a task this set owns: a caller that stops waiting (a
//! screen closed, a phone's job cancelled) doesn't stop the step, a caller
//! that asks again waits for the same step, and `cancel_link` stops it.
//! Every step has an id: a step settles the slot, or opens a workspace, only
//! while the slot still holds that step, so a step that was stopped or put
//! aside (`close_all`) changes nothing afterwards.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use tokio::sync::watch;

use super::Workspaces;
use crate::{
    membership::{
        invite::JoinMailbox,
        link::{LinkCode, Platform, Token},
    },
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
const PUT_ASIDE: &str = "The app closed this device's link; it picks up again at the next start.";
const STEP_FAILED: &str =
    "Linking hit an unexpected problem on this device. Open the app again to pick the link up.";

static NEXT_STEP_ID: AtomicU64 = AtomicU64::new(1);

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
    id: u64,
    step: Step,
    /// Stops the step, which then wipes what the link holds.
    cancel: JoinCancel,
    /// Set once the step ends, for every caller waiting on it.
    outcome: watch::Receiver<Option<StepOutcome>>,
    /// Drops the step without wiping, as quitting does; set once it runs.
    abort: Option<tokio::task::AbortHandle>,
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// Joined only by a caller with the same code.
    Begin(Token),
    Handover,
    Added,
    /// Wiping a link that was between steps.
    Stop,
}

#[derive(Clone)]
enum StepOutcome {
    Check(CheckOnNewDevice),
    Opened(Workspace),
    Stopped,
    Failed(String),
}

fn held(linking: DeviceLinking<LinkMailbox>, reading: Option<Workspace>) -> LinkSlot {
    LinkSlot::Held(Box::new(HeldLink { linking, reading }))
}

/// What a restart found: a link still waiting for its hand-over, with the
/// number to show if the person's device opened the request already (none
/// while the storage is out of reach), or one reading, whose workspace is
/// open and selected.
pub enum ResumedLinkStep {
    Waiting { code: Option<String> },
    Reading(Workspace),
}

/// How a stop ended: the link stopped and was wiped, or the admin's app
/// had added the device just before, and the joined workspace stays.
pub enum LinkStop {
    Stopped,
    AlreadyJoined(Workspace),
}

/// How a new device reaches its code's slots: the code's presigned links,
/// or the in-memory bucket in tests.
pub(crate) enum LinkMailbox {
    Presigned(PresignedMailbox),
    #[cfg(test)]
    Bucket(crate::tests::support::FlakyLinkMailbox),
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

/// What tests make this set's links do.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestLinkHooks {
    /// Links reach this bucket directly, as the in-memory bucket can't presign.
    pub bucket: Option<crate::transport::bucket::Bucket>,
    /// How many request posts and answer fetches fail, for mailboxes made
    /// from now on.
    pub post_failures: usize,
    pub fetch_failures: usize,
    /// Each step, once done, says so and waits for a permit before it settles.
    pub settle_gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Semaphore>)>,
    /// The next step panics.
    pub panics_next_step: bool,
}

/// What a caller does with a step, decided under the slot's lock.
enum Next {
    /// The step runs already: wait for it.
    Join(watch::Receiver<Option<StepOutcome>>),
    Start(StartedStep),
}

/// A step just marked running: `taken` is what the slot held, to hand to
/// the step or put back.
struct StartedStep {
    id: u64,
    taken: LinkSlot,
    cancel: JoinCancel,
    tell: watch::Sender<Option<StepOutcome>>,
    outcome: watch::Receiver<Option<StepOutcome>>,
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
    /// disk after a restart: one link at a time. A second call with the same
    /// code waits for the same step.
    pub async fn begin_link(self: &Arc<Self>, code: &str, platform: Platform) -> Result<CheckOnNewDevice> {
        let decoded = read_code(code)?;
        if self.linking_folder()?.is_some() {
            self.refuse_begin_beside_kept_link()?;
        }
        let outcome = match self.next_step(Step::Begin(decoded.token))? {
            Next::Join(outcome) => outcome,
            Next::Start(StartedStep {
                id,
                cancel,
                tell,
                outcome,
                ..
            }) => {
                let data = match self.prepare_folder(&decoded.workspace_id) {
                    Ok(data) => data,
                    Err(error) => {
                        self.settle(id, LinkSlot::Idle);
                        return Err(error);
                    }
                };
                let mailbox = self.link_mailbox(&decoded);
                self.spawn_step(id, tell, None, async move {
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
            Next::Start(StartedStep {
                id,
                taken,
                cancel,
                tell,
                outcome,
            }) => {
                let HeldLink { mut linking, reading } = self.take_held(id, taken)?;
                if let Some(reading) = reading {
                    self.settle(id, held(linking, Some(reading)));
                    bail!("this device is reading already: wait until it is added");
                }
                let connect = ReadingConnector::over(self.connect.clone());
                let workspaces = self.clone();
                self.spawn_step(id, tell, None, async move {
                    match linking.wait_for_handover_with(&cancel, connect).await {
                        Ok(reading) => match workspaces.open_for_step(id, reading.clone()) {
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
            Next::Start(StartedStep {
                id,
                taken,
                cancel,
                tell,
                outcome,
            }) => {
                let HeldLink { linking, reading } = self.take_held(id, taken)?;
                let Some(reading) = reading else {
                    self.settle(id, held(linking, None));
                    bail!("this device isn't reading yet: wait for the hand-over first");
                };
                let connect = self.connect.clone();
                let workspaces = self.clone();
                let reading_id = Some(reading.workspace_id().to_owned());
                self.spawn_step(id, tell, reading_id, async move {
                    let workspace_id = reading.workspace_id().to_owned();
                    let register = {
                        let workspaces = workspaces.clone();
                        let workspace_id = workspace_id.clone();
                        move |joined: Workspace| workspaces.swap_in_joined(id, &workspace_id, joined)
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
    /// restart. Says so if the admin's app added the device just before.
    /// Never for shutting the app down: quitting keeps the link, and
    /// `resume_link` picks it up at the next start.
    pub async fn cancel_link(self: &Arc<Self>) -> Result<LinkStop> {
        loop {
            let (outcome, is_own_stop) = match self.stop_or_wipe()? {
                Some(waiting) => waiting,
                None => match self.resume_link().await? {
                    Some(_) => continue,
                    None => bail!("there's no link to stop on this device"),
                },
            };
            let ended = wait_for(outcome).await?;
            // A step that ended just before it saw the stop leaves the link
            // between steps: stop that too.
            if matches!(*self.link_lock(), LinkSlot::Held(_)) {
                continue;
            }
            return match ended {
                StepOutcome::Opened(joined) if !joined.is_reading_only() => {
                    Ok(LinkStop::AlreadyJoined(joined))
                }
                StepOutcome::Failed(message) if is_own_stop => Err(anyhow!(message)),
                _ => Ok(LinkStop::Stopped),
            };
        }
    }

    /// Under the slot's lock: stops a running step, or starts wiping a link
    /// between steps, and returns what to wait for, and whether it is this
    /// call's own wipe. `None` when there's no link in this set.
    fn stop_or_wipe(self: &Arc<Self>) -> Result<Option<(watch::Receiver<Option<StepOutcome>>, bool)>> {
        let started = {
            let mut slot = self.link_lock();
            match &*slot {
                LinkSlot::Idle => return Ok(None),
                LinkSlot::Running(running) => {
                    running.cancel.cancel();
                    return Ok(Some((running.outcome.clone(), false)));
                }
                LinkSlot::Held(_) => start_step(&mut slot, Step::Stop),
            }
        };
        let StartedStep {
            id,
            taken,
            tell,
            outcome,
            ..
        } = started;
        let HeldLink { linking, reading } = self.take_held(id, taken)?;
        let workspaces = self.clone();
        let reading_id = reading.as_ref().map(|reading| reading.workspace_id().to_owned());
        self.spawn_step(id, tell, reading_id, async move {
            if let Some(reading) = &reading {
                workspaces.close_reading_workspace(reading.workspace_id());
            }
            match linking.cancel(reading).await {
                Ok(()) => (LinkSlot::Idle, StepOutcome::Stopped),
                Err(error) => (LinkSlot::Idle, failed(&error)),
            }
        });
        Ok(Some((outcome, true)))
    }

    /// Picks up a link a restart interrupted, if there is one; call it after
    /// `open_all`. `None` when there is nothing to pick up, or a link is
    /// already in progress here. One that began joining, waited a week,
    /// was stopped by the person's device or answered someone else is
    /// wiped, with the error to show; storage out of reach wipes nothing.
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
            Some(ResumedLink::Waiting(mut linking)) => {
                let code = linking.check_after_restart().await?;
                self.set_link_slot(held(linking, None));
                Ok(Some(ResumedLinkStep::Waiting { code }))
            }
            Some(ResumedLink::Reading(linking, reading)) => {
                self.set_link_slot(held(linking, Some(reading.clone())));
                Ok(Some(ResumedLinkStep::Reading(self.add_open(reading)?)))
            }
        }
    }

    /// Puts this device's link aside as the app closes: a running step is
    /// dropped, not stopped, so the link stays on disk for the next start,
    /// and nothing it was doing reaches this set afterwards.
    pub(super) fn put_link_aside(&self) {
        let taken = std::mem::take(&mut *self.link_lock());
        if let LinkSlot::Running(RunningStep {
            abort: Some(abort), ..
        }) = &taken
        {
            abort.abort();
        }
    }

    /// Refuses a new link beside one kept on disk, naming its workspace if
    /// this set holds it.
    fn refuse_begin_beside_kept_link(&self) -> Result<()> {
        let slot = self.link_lock();
        let name = match &*slot {
            LinkSlot::Running(RunningStep {
                step: Step::Begin(_), ..
            }) => return Ok(()),
            LinkSlot::Held(held_link) => format!("\u{201c}{}\u{201d}", held_link.linking.workspace_name()),
            LinkSlot::Running(_) | LinkSlot::Idle => "a workspace".to_owned(),
        };
        bail!("This device is already linking to {name}. Pick that link up, or stop it, first.")
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
            LinkSlot::Held(_) if matches!(step, Step::Begin(_)) => {
                bail!("this device is already linking; finish or stop that first")
            }
            LinkSlot::Idle if !matches!(step, Step::Begin(_)) => bail!(NO_LINK_WAITING),
            _ => {}
        }
        Ok(Next::Start(start_step(&mut slot, step)))
    }

    /// What a step between steps works on, or why there's none (the slot
    /// goes back as it was).
    fn take_held(&self, id: u64, taken: LinkSlot) -> Result<HeldLink> {
        match taken {
            LinkSlot::Held(held_link) => Ok(*held_link),
            other => {
                self.settle(id, other);
                bail!(NO_LINK_WAITING)
            }
        }
    }

    /// Runs a step in a task this set owns: dropping the caller's future
    /// doesn't stop it. As it ends it settles the slot, if the slot still
    /// holds it, then tells every caller waiting. A step that panics leaves
    /// the link on disk to pick up again, and says so; the reading workspace
    /// it held (`reading_id`) is closed, so picking up can open it again.
    fn spawn_step(
        self: &Arc<Self>,
        id: u64,
        tell: watch::Sender<Option<StepOutcome>>,
        reading_id: Option<String>,
        step: impl std::future::Future<Output = (LinkSlot, StepOutcome)> + Send + 'static,
    ) {
        #[cfg(test)]
        let step = self.with_test_hooks(step);
        let running = {
            let mut slot = self.link_lock();
            let LinkSlot::Running(running) = &mut *slot else {
                tell.send_replace(Some(StepOutcome::Failed(PUT_ASIDE.to_owned())));
                return;
            };
            if running.id != id {
                tell.send_replace(Some(StepOutcome::Failed(PUT_ASIDE.to_owned())));
                return;
            }
            let task = tokio::spawn(step);
            running.abort = Some(task.abort_handle());
            task
        };
        let workspaces = self.clone();
        tokio::spawn(async move {
            let (next, ended) = match running.await {
                Ok(done) => done,
                Err(error) if error.is_cancelled() => {
                    (LinkSlot::Idle, StepOutcome::Failed(PUT_ASIDE.to_owned()))
                }
                Err(error) => {
                    tracing::error!("a link step stopped unexpectedly: {error}");
                    if let Some(reading_id) = &reading_id {
                        workspaces.close_reading_workspace(reading_id);
                    }
                    (LinkSlot::Idle, StepOutcome::Failed(STEP_FAILED.to_owned()))
                }
            };
            workspaces.settle(id, next);
            tell.send_replace(Some(ended));
        });
    }

    /// Leaves the slot at `next` if it still holds step `id`; otherwise the
    /// step was stopped or put aside, and `next` is dropped (a dropped link
    /// stays on disk).
    fn settle(&self, id: u64, next: LinkSlot) {
        let mut slot = self.link_lock();
        if matches!(&*slot, LinkSlot::Running(running) if running.id == id) {
            *slot = next;
        }
    }

    /// Whether the slot still holds step `id`, for a step about to change
    /// the open workspaces: checked under the slot's lock, held while it
    /// changes them, so a step put aside opens nothing.
    fn while_step_runs<T>(&self, id: u64, change: impl FnOnce() -> Result<T>) -> Result<T> {
        let slot = self.link_lock();
        ensure!(
            matches!(&*slot, LinkSlot::Running(running) if running.id == id),
            PUT_ASIDE
        );
        change()
    }

    fn open_for_step(&self, id: u64, reading: Workspace) -> Result<Workspace> {
        self.while_step_runs(id, || self.add_open(reading))
    }

    /// Puts the joined workspace in the reading one's place, under one lock,
    /// and selects it.
    fn swap_in_joined(&self, id: u64, workspace_id: &str, joined: Workspace) -> Result<Workspace> {
        self.while_step_runs(id, || {
            {
                let mut open = self.open_lock();
                open.retain(|workspace| {
                    !(workspace.workspace_id() == workspace_id && workspace.is_reading_only())
                });
                open.push(joined);
            }
            self.select(workspace_id)
        })
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

    /// Changes what tests make this set's links do.
    #[cfg(test)]
    pub(crate) fn change_test_link_hooks(&self, change: impl FnOnce(&mut TestLinkHooks)) {
        if let Ok(mut hooks) = self.test_link_hooks.lock() {
            change(&mut hooks);
        }
    }

    /// Links reach `bucket` directly, as the in-memory bucket can't
    /// presign.
    #[cfg(test)]
    pub(crate) fn set_test_link_bucket(&self, bucket: crate::transport::bucket::Bucket) {
        self.change_test_link_hooks(|hooks| hooks.bucket = Some(bucket));
    }

    /// `step`, made to panic, or to wait at the settle gate, as the test
    /// hooks say.
    #[cfg(test)]
    fn with_test_hooks(
        &self,
        step: impl std::future::Future<Output = (LinkSlot, StepOutcome)> + Send + 'static,
    ) -> impl std::future::Future<Output = (LinkSlot, StepOutcome)> + Send + 'static {
        let (panics, gate) = match self.test_link_hooks.lock() {
            Ok(mut hooks) => (
                std::mem::take(&mut hooks.panics_next_step),
                hooks.settle_gate.clone(),
            ),
            Err(_) => (false, None),
        };
        async move {
            assert!(!panics, "a simulated panic in a link step");
            let ended = step.await;
            if let Some((reached, release)) = gate {
                reached.notify_one();
                drop(release.acquire().await);
            }
            ended
        }
    }

    fn link_mailbox(&self, code: &LinkCode) -> LinkMailbox {
        #[cfg(test)]
        if let Some((bucket, post_failures, fetch_failures)) = self
            .test_link_hooks
            .lock()
            .ok()
            .and_then(|hooks| Some((hooks.bucket.clone()?, hooks.post_failures, hooks.fetch_failures)))
        {
            return LinkMailbox::Bucket(crate::tests::support::FlakyLinkMailbox::new(
                bucket,
                code.token,
                post_failures,
                fetch_failures,
            ));
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

/// Marks `step` running in `slot`, with a fresh id, and hands back what
/// the slot held.
fn start_step(slot: &mut LinkSlot, step: Step) -> StartedStep {
    let id = NEXT_STEP_ID.fetch_add(1, Ordering::Relaxed);
    let cancel = JoinCancel::new();
    let (tell, outcome) = watch::channel(None);
    let running = LinkSlot::Running(RunningStep {
        id,
        step,
        cancel: cancel.clone(),
        outcome: outcome.clone(),
        abort: None,
    });
    StartedStep {
        id,
        taken: std::mem::replace(slot, running),
        cancel,
        tell,
        outcome,
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
