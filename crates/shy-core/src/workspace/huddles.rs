//! Huddles from the workspace's side: joining and leaving, beacons sent and
//! received, the rosters, and keeping the media layer's audience to exactly
//! the people who may hear this device. All of it is in memory; nothing about
//! a huddle reaches the op log or the bucket.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use tokio::sync::{mpsc, watch};
use tracing::warn;

use super::Workspace;
use crate::{
    huddle::{
        beacon::{Beacon, BeaconBody, BeaconState, MEDIA_WIRE, Reach, SessionId, media_path_name},
        book::{EXPIRY_MS, HuddleBook, REFRESH_MS},
        media::{DeviceChoice, Devices, Link, MediaCall, MediaEvent, MicState, OutputState},
        rules::{MAX_PARTICIPANTS, Visibility, hearing_set, huddle_members},
    },
    ops::now_ms,
    transport::direct::DirectMessage,
    views::{Change, Huddle, HuddleParticipant, HuddlePeer, HuddlePhase, MyHuddle, member_id},
};

/// How often the session checks membership and whether the device slept.
const TICK: Duration = Duration::from_secs(5);
/// A `Left` is sent this many times, since gossip may lose one.
const LEFT_REPEATS: usize = 2;

/// Where a beacon came from: gossip, or straight from a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Gossip,
    Direct(PublicKey),
}

/// Every huddle this device knows of, and its own.
pub(crate) struct HuddleState {
    book: HuddleBook,
    mine: Option<MySession>,
    /// The origin of the monotonic clock rosters expire by.
    started: Instant,
    /// Beacon sends still on their way, so closing can wait for them.
    sending: Vec<tokio::task::JoinHandle<()>>,
    /// Every beacon this device sent, for tests to carry between members.
    #[cfg(test)]
    sent: Vec<Beacon>,
}

impl Default for HuddleState {
    fn default() -> Self {
        Self {
            book: HuddleBook::default(),
            mine: None,
            started: Instant::now(),
            sending: Vec::new(),
            #[cfg(test)]
            sent: Vec::new(),
        }
    }
}

impl HuddleState {
    fn now(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// What recomputing the audience found.
enum Audience {
    Updated,
    /// This device may no longer be in its session's huddle.
    Lost(SessionId),
}

struct MySession {
    channel: String,
    session: SessionId,
    seq: u64,
    visibility: Visibility,
    muted: bool,
    phase: HuddlePhase,
    mic: MicState,
    output: OutputState,
    links: BTreeMap<PublicKey, Link>,
    speaking: BTreeSet<PublicKey>,
    peers: watch::Sender<BTreeMap<PublicKey, String>>,
    task: tokio::task::AbortHandle,
}

impl Workspace {
    fn huddles(&self) -> std::sync::MutexGuard<'_, HuddleState> {
        self.inner
            .huddles
            .lock()
            .expect("huddle state lock poisoned by an earlier panic")
    }

    /// Who is in `channel`'s huddle, if this member may see the channel.
    pub fn huddle(&self, channel: &str) -> Result<Huddle> {
        let members = self.current_members()?;
        let allowed = {
            let state = self.state();
            huddle_members(&state.store, channel, &members)?
                .context("no such channel here")?
                .0
        };
        ensure!(
            allowed.contains(&self.me()),
            "you aren't a member of this channel"
        );
        let huddles = self.huddles();
        let now = huddles.now();
        let mut participants: Vec<HuddleParticipant> = huddles
            .book
            .roster(channel, now)
            .into_iter()
            .filter(|participant| allowed.contains(&participant.member) && participant.member != self.me())
            .map(|participant| HuddleParticipant {
                member: member_id(&participant.member),
                camera: participant.camera,
                screen: participant.screen,
                is_me: false,
                needs_update: participant.media_wire != MEDIA_WIRE,
            })
            .collect();
        if huddles.mine.as_ref().is_some_and(|mine| mine.channel == channel) {
            participants.insert(
                0,
                HuddleParticipant {
                    member: self.my_id(),
                    camera: false,
                    screen: false,
                    is_me: true,
                    needs_update: false,
                },
            );
        }
        Ok(Huddle {
            channel: channel.to_owned(),
            is_full: participants.len() >= MAX_PARTICIPANTS,
            participants,
        })
    }

    /// This device's own huddle, if it is in one.
    pub fn my_huddle(&self) -> Option<MyHuddle> {
        let huddles = self.huddles();
        let mine = huddles.mine.as_ref()?;
        let peers = mine.peers.borrow();
        Some(MyHuddle {
            channel: mine.channel.clone(),
            phase: mine.phase,
            muted: mine.muted,
            mic: mine.mic,
            output: mine.output,
            peers: peers
                .keys()
                .map(|peer| HuddlePeer {
                    member: member_id(peer),
                    link: mine.links.get(peer).copied().unwrap_or(Link::Connecting),
                    speaking: mine.speaking.contains(peer),
                })
                .collect(),
        })
    }

    /// Joins and leaves happen one at a time, so the audio and the state
    /// never disagree. Neither waits on the network while holding this.
    fn transition(&self) -> std::sync::MutexGuard<'_, ()> {
        self.inner
            .huddle_transition
            .lock()
            .expect("huddle transition lock poisoned by an earlier panic")
    }

    /// Joins `channel`'s huddle, leaving any other first.
    pub fn join_huddle(&self, channel: &str, muted: bool) -> Result<()> {
        let media = self
            .inner
            .huddle_media
            .clone()
            .context("this build of shyspace can't join huddles: it has no audio")?;
        let _transition = self.transition();
        self.leave_current()?;
        let members = self.current_members()?;
        let (allowed, visibility) = {
            let state = self.state();
            huddle_members(&state.store, channel, &members)?.context("no such channel here")?
        };
        ensure!(
            allowed.contains(&self.me()),
            "you aren't a member of this channel"
        );
        ensure!(
            self.huddle(channel)?.participants.len() < MAX_PARTICIPANTS,
            "this huddle is full"
        );

        let session: SessionId = rand::random();
        let beacon = self.sign_beacon(channel, session, 0, joined())?;
        let (events, event_receiver) = mpsc::channel(64);
        let (peers, peer_receiver) = watch::channel(BTreeMap::new());
        let task = self
            .inner
            .runtime
            .spawn(self.clone().run_session(session, event_receiver))
            .abort_handle();
        self.huddles().mine = Some(MySession {
            channel: channel.to_owned(),
            session,
            seq: 0,
            visibility,
            muted,
            phase: HuddlePhase::Joining,
            mic: if muted { MicState::Off } else { MicState::Opening },
            output: OutputState::Playing,
            links: BTreeMap::new(),
            speaking: BTreeSet::new(),
            peers,
            task,
        });
        let started = match self.refresh_audience() {
            Ok(Audience::Updated) => media
                .start(MediaCall {
                    my_path: media_path_name(&session),
                    peers: peer_receiver,
                    events,
                    muted,
                    devices: DeviceChoice::default(),
                })
                .context("couldn't start the huddle's audio"),
            Ok(Audience::Lost(_)) => Err(anyhow::anyhow!("you aren't a member of this channel")),
            Err(error) => Err(error),
        };
        if let Err(error) = started {
            if let Some(mine) = self.take_my_huddle() {
                mine.task.abort();
            }
            return Err(error);
        }
        if let Some(mine) = self.huddles().mine.as_mut() {
            mine.phase = HuddlePhase::In;
        }
        self.send_beacon(beacon, visibility, &allowed);
        self.emit(Change::MyHuddle);
        self.emit(Change::Huddle {
            channel: channel.to_owned(),
        });
        Ok(())
    }

    /// Leaves this device's huddle, if any: the audio stops at once, and the
    /// others are told in the background.
    pub fn leave_huddle(&self) -> Result<()> {
        let _transition = self.transition();
        self.leave_current()
    }

    /// Leaves `session` if it's still this device's; a newer one stays.
    fn leave_session(&self, session: SessionId) -> Result<()> {
        let _transition = self.transition();
        if self
            .huddles()
            .mine
            .as_ref()
            .is_some_and(|mine| mine.session == session)
        {
            self.leave_current()?;
        }
        Ok(())
    }

    /// Only with the transition lock held.
    fn leave_current(&self) -> Result<()> {
        let Some(ended) = self.take_my_huddle() else {
            return Ok(());
        };
        ended.task.abort();
        self.emit(Change::MyHuddle);
        self.emit(Change::Huddle {
            channel: ended.channel.clone(),
        });
        let members = self.current_members()?;
        let recipients = {
            let state = self.state();
            huddle_members(&state.store, &ended.channel, &members)?.map(|(allowed, _)| allowed)
        };
        for repeat in 0..LEFT_REPEATS as u64 {
            let beacon = self.sign_beacon(
                &ended.channel,
                ended.session,
                ended.seq + 1 + repeat,
                BeaconState::Left,
            )?;
            self.send_beacon(
                beacon,
                ended.visibility,
                recipients.as_deref().unwrap_or_default(),
            );
        }
        Ok(())
    }

    /// Waits until the beacons already sent are on their way, so a `Left`
    /// isn't cut off by closing.
    pub(crate) async fn finish_huddle_sends(&self) {
        let sending = std::mem::take(&mut self.huddles().sending);
        for send in sending {
            if let Err(error) = send.await {
                warn!("a huddle beacon send failed: {error}");
            }
        }
    }

    /// Mutes or unmutes this device. Muting closes the microphone, so the
    /// system's indicator goes dark too.
    pub fn set_huddle_muted(&self, muted: bool) -> Result<()> {
        let media = self
            .inner
            .huddle_media
            .clone()
            .context("this build has no audio")?;
        ensure!(self.my_huddle().is_some(), "you aren't in a huddle");
        media.set_muted(muted)?;
        if let Some(mine) = self.huddles().mine.as_mut() {
            mine.muted = muted;
            mine.mic = if muted { MicState::Off } else { MicState::Opening };
        }
        self.emit(Change::MyHuddle);
        Ok(())
    }

    pub fn huddle_devices(&self) -> Result<Devices> {
        self.inner
            .huddle_media
            .as_ref()
            .context("this build has no audio")?
            .list_devices()
    }

    pub fn set_huddle_devices(&self, choice: DeviceChoice) -> Result<()> {
        self.inner
            .huddle_media
            .as_ref()
            .context("this build has no audio")?
            .set_devices(choice)
    }

    /// Takes a beacon from gossip or straight from a device, after every check.
    pub fn receive_beacon(&self, beacon: &Beacon, via: Via) -> Result<()> {
        let body = &beacon.body;
        beacon.verify(&self.inner.config.workspace_id)?;
        if let Via::Direct(from) = via {
            ensure!(
                from == body.endpoint,
                "a beacon sent by another device than it names"
            );
        }
        if body.member == self.me() {
            return Ok(());
        }
        let members = self.current_members()?;
        let (allowed, visibility) = {
            let state = self.state();
            huddle_members(&state.store, &body.channel, &members)?
                .context("a beacon for a channel not here")?
        };
        ensure!(
            allowed.contains(&body.member),
            "a beacon from someone who may not be in this huddle"
        );
        ensure!(
            visibility == Visibility::Workspace || via != Via::Gossip,
            "a private huddle's beacon arrived over gossip"
        );
        let wall_ms = now_ms()?;
        let (expired, received) = {
            let mut huddles = self.huddles();
            let now = huddles.now();
            let expired = huddles.book.forget_expired(now);
            (expired, huddles.book.receive(beacon, wall_ms, now))
        };
        self.after_roster_change(expired.channels)?;
        self.after_roster_change(received?.channels)
    }

    /// The beacons this device sent since the last call.
    #[cfg(test)]
    pub(crate) fn take_sent_beacons(&self) -> Vec<Beacon> {
        std::mem::take(&mut self.huddles().sent)
    }

    fn after_roster_change(&self, channels: Vec<String>) -> Result<()> {
        if channels.is_empty() {
            return Ok(());
        }
        let mine = self.huddles().mine.as_ref().map(|mine| mine.channel.clone());
        if mine.as_ref().is_some_and(|channel| channels.contains(channel)) {
            self.follow_membership()?;
            self.emit(Change::MyHuddle);
        }
        for channel in channels {
            self.emit(Change::Huddle { channel });
        }
        Ok(())
    }

    /// Updates whom this device exchanges audio with, and leaves if it may
    /// no longer be in the huddle.
    fn follow_membership(&self) -> Result<()> {
        match self.refresh_audience()? {
            Audience::Updated => Ok(()),
            Audience::Lost(session) => self.leave_session(session),
        }
    }

    /// Recomputes whom this device exchanges audio with, from the roster
    /// and who may be in the huddle now. Someone removed drops out here.
    fn refresh_audience(&self) -> Result<Audience> {
        let Some((channel, session)) = self
            .huddles()
            .mine
            .as_ref()
            .map(|mine| (mine.channel.clone(), mine.session))
        else {
            return Ok(Audience::Updated);
        };
        let members = self.current_members()?;
        let allowed = {
            let state = self.state();
            huddle_members(&state.store, &channel, &members)?.map(|(allowed, _)| allowed)
        };
        let me = self.me();
        let mut huddles = self.huddles();
        let now = huddles.now();
        let roster = huddles.book.roster(&channel, now);
        let Some(mine) = huddles.mine.as_mut().filter(|mine| mine.session == session) else {
            // A newer session took over meanwhile; it has its own audience.
            return Ok(Audience::Updated);
        };
        let Some(allowed) = allowed.filter(|allowed| allowed.contains(&me)) else {
            // Removed from the channel, or it's gone.
            return Ok(Audience::Lost(session));
        };
        let heard = hearing_set(&roster, &me, &allowed);
        mine.links.retain(|peer, _| heard.contains_key(peer));
        mine.speaking.retain(|peer| heard.contains_key(peer));
        mine.peers.send_if_modified(|peers| {
            let is_changed = *peers != heard;
            *peers = heard;
            is_changed
        });
        Ok(Audience::Updated)
    }

    /// Ends this device's session locally: no state left, no audio.
    fn take_my_huddle(&self) -> Option<MySession> {
        let ended = self.huddles().mine.take()?;
        if let Some(media) = &self.inner.huddle_media {
            media.stop();
        }
        Some(ended)
    }

    fn sign_beacon(&self, channel: &str, session: SessionId, seq: u64, state: BeaconState) -> Result<Beacon> {
        Beacon::sign(
            &self.inner.identity,
            &self.inner.config.workspace_id,
            BeaconBody {
                member: self.me(),
                endpoint: self.me(),
                channel: channel.to_owned(),
                session,
                seq,
                sent_ms: now_ms()?,
                state,
                media_wire: MEDIA_WIRE,
            },
        )
    }

    /// Sends a beacon the way its channel's huddle requires: over gossip, or
    /// to each of the channel's members. Sending happens in the background,
    /// so nobody slow or offline holds anything up; a failure is logged, and
    /// the next refresh tries again. Beacons can arrive out of order: their
    /// counter puts them right.
    fn send_beacon(&self, beacon: Beacon, visibility: Visibility, allowed: &[PublicKey]) {
        let mut huddles = self.huddles();
        #[cfg(test)]
        huddles.sent.push(beacon.clone());
        if let Some(mine) = huddles.mine.as_mut()
            && mine.session == beacon.body.session
        {
            mine.seq = mine.seq.max(beacon.body.seq);
        }
        if self.inner.net.is_none() {
            return;
        }
        let me = self.me();
        let recipients: Vec<PublicKey> = allowed.iter().filter(|member| **member != me).copied().collect();
        let workspace = self.clone();
        let send = self.inner.runtime.spawn(async move {
            let Some(net) = &workspace.inner.net else { return };
            if visibility == Visibility::Workspace {
                if let Err(error) = net.broadcast_beacon(beacon).await {
                    warn!("couldn't send a huddle beacon: {error:#}");
                }
                return;
            }
            let message = DirectMessage::Huddle(beacon);
            let mut sends = tokio::task::JoinSet::new();
            for member in recipients {
                let (workspace, message) = (workspace.clone(), message.clone());
                sends.spawn(async move {
                    let Some(net) = &workspace.inner.net else {
                        return Ok(());
                    };
                    net.send_direct(member, &message).await
                });
            }
            while let Some(sent) = sends.join_next().await {
                match sent {
                    Ok(Ok(())) => {}
                    // Offline members are expected; they catch up on their next refresh.
                    Ok(Err(error)) => {
                        tracing::debug!("couldn't reach a member with a huddle beacon: {error:#}")
                    }
                    Err(error) => warn!("a huddle beacon send failed: {error}"),
                }
            }
        });
        huddles.sending.retain(|send| !send.is_finished());
        huddles.sending.push(send);
    }

    /// Runs while this device is in a huddle: refreshes its beacon, follows
    /// membership, applies the media layer's reports, and leaves after a sleep.
    async fn run_session(self, session: SessionId, mut events: mpsc::Receiver<MediaEvent>) {
        let mut changes = self.subscribe();
        let mut tick = tokio::time::interval(TICK);
        let mut last_tick = Instant::now();
        let mut last_refresh = Instant::now();
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Some(event) => self.apply_media_event(event),
                    None => return,
                },
                change = changes.recv() => {
                    if matches!(change, Ok(Change::Members | Change::Channels) | Err(_))
                        && let Err(error) = self.follow_membership()
                    {
                        warn!("couldn't update a huddle's audience: {error:#}");
                    }
                }
                _ = tick.tick() => {
                    // A device that slept past expiry has left, for everyone else.
                    if last_tick.elapsed() >= Duration::from_millis(EXPIRY_MS) {
                        if let Err(error) = self.leave_session(session) {
                            warn!("couldn't leave a huddle after sleeping: {error:#}");
                        }
                        return;
                    }
                    last_tick = Instant::now();
                    // Membership can change without a local event, like a
                    // removal learned from the bucket.
                    if let Err(error) = self.follow_membership() {
                        warn!("couldn't update a huddle's audience: {error:#}");
                    }
                    if last_refresh.elapsed() >= Duration::from_millis(REFRESH_MS) {
                        last_refresh = Instant::now();
                        if let Err(error) = self.refresh_beacon(session) {
                            warn!("couldn't refresh a huddle beacon: {error:#}");
                        }
                    }
                }
            }
        }
    }

    fn refresh_beacon(&self, session: SessionId) -> Result<()> {
        let Some((channel, seq, visibility)) = self
            .huddles()
            .mine
            .as_ref()
            .filter(|mine| mine.session == session)
            .map(|mine| (mine.channel.clone(), mine.seq, mine.visibility))
        else {
            return Ok(());
        };
        let members = self.current_members()?;
        let allowed = {
            let state = self.state();
            huddle_members(&state.store, &channel, &members)?.map(|(allowed, _)| allowed)
        };
        match allowed {
            Some(allowed) if allowed.contains(&self.me()) => {
                let beacon = self.sign_beacon(&channel, session, seq + 1, joined())?;
                self.send_beacon(beacon, visibility, &allowed);
                Ok(())
            }
            // Removed from the channel, or it's gone: nothing left to be in.
            _ => self.leave_session(session),
        }
    }

    /// Drops participants whose refreshes stopped, and says so. Runs while
    /// online, whether or not this device is in a huddle.
    pub(crate) fn expire_huddle_participants(&self) -> Result<()> {
        let expired = {
            let mut huddles = self.huddles();
            let now = huddles.now();
            huddles.book.forget_expired(now)
        };
        self.after_roster_change(expired.channels)
    }

    fn apply_media_event(&self, event: MediaEvent) {
        let mut speaking_change = None;
        {
            let mut huddles = self.huddles();
            let Some(mine) = huddles.mine.as_mut() else { return };
            match event {
                MediaEvent::Link { peer, link } => {
                    if mine.peers.borrow().contains_key(&peer) {
                        mine.links.insert(peer, link);
                    }
                }
                MediaEvent::Speaking { peer, speaking } => {
                    if !mine.peers.borrow().contains_key(&peer) {
                        return;
                    }
                    let is_change = if speaking {
                        mine.speaking.insert(peer)
                    } else {
                        mine.speaking.remove(&peer)
                    };
                    if !is_change {
                        return;
                    }
                    speaking_change = Some((member_id(&peer), speaking));
                }
                MediaEvent::MeSpeaking(speaking) => speaking_change = Some((self.my_id(), speaking)),
                MediaEvent::Mic(mic) => mine.mic = mic,
                MediaEvent::Output(output) => mine.output = output,
                MediaEvent::Incompatible { .. } => {}
            }
        }
        match speaking_change {
            Some((member, speaking)) => self.emit(Change::HuddleSpeaking { member, speaking }),
            None => self.emit(Change::MyHuddle),
        }
    }
}

fn joined() -> BeaconState {
    BeaconState::Joined {
        camera: false,
        screen: false,
        reach: Reach::Direct,
    }
}
