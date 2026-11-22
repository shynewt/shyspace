//! Who is in which huddle, from verified beacons, in memory only. A device's
//! beacons are ordered within its session by `seq`, so a clock that steps
//! backwards mid-call changes nothing; a new session must fall within the
//! clock skew and can't be one that already ended. Entries expire when their
//! refreshes stop (a crash, a lost network).

use std::collections::HashMap;

use anyhow::{Context, Result, bail, ensure};
use iroh::PublicKey;

use super::beacon::{Beacon, BeaconState, Reach, SessionId};

/// A beacon this far from our clock is refused, so a recorded one can't be
/// replayed later.
pub const MAX_CLOCK_SKEW_MS: i64 = 2 * 60 * 1000;
/// How often a participant re-sends its beacon.
pub const REFRESH_MS: u64 = 30 * 1000;
/// Without a refresh for this long, a participant has left.
pub const EXPIRY_MS: u64 = 90 * 1000;
/// Joins and leaves from one device closer together than this are refused,
/// so nobody can make everyone's roster redraw nonstop.
const MIN_CHANGE_GAP_MS: u64 = 1000;
/// Ended sessions remembered per device. A real device ends a few per
/// minute; a hostile one sending leaves for made-up sessions only pushes out
/// its own older ones.
pub const MAX_RETIRED_PER_DEVICE: usize = 16;

/// One device in a huddle, as others see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub member: PublicKey,
    pub endpoint: PublicKey,
    pub channel: String,
    pub session: SessionId,
    pub camera: bool,
    pub screen: bool,
    pub reach: Reach,
    pub media_wire: u16,
}

struct Entry {
    participant: Participant,
    seq: u64,
    sent_ms: i64,
    heard_at: u64,
}

/// What a beacon changed: the channels whose roster is now different.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changed {
    pub channels: Vec<String>,
}

#[derive(Default)]
pub struct HuddleBook {
    entries: HashMap<PublicKey, Entry>,
    /// Sessions that ended, per device, until when (our monotonic clock) a
    /// replay of them is still possible.
    retired: HashMap<PublicKey, Vec<(SessionId, u64)>>,
    last_change: HashMap<PublicKey, u64>,
}

impl HuddleBook {
    /// Takes a beacon that was already verified and whose sender may be in
    /// its channel's huddle. `wall_ms` is our clock, `now` a monotonic one.
    /// Call `forget_expired` first, so a silent participant's late beacon
    /// starts afresh and the expiry is reported even if this one is refused.
    pub fn receive(&mut self, beacon: &Beacon, wall_ms: i64, now: u64) -> Result<Changed> {
        ensure!(
            (beacon.body.sent_ms - wall_ms).abs() <= MAX_CLOCK_SKEW_MS,
            "beacon too old or from the future"
        );
        self.apply(beacon, now)
    }

    fn apply(&mut self, beacon: &Beacon, now: u64) -> Result<Changed> {
        let body = &beacon.body;
        let device = body.endpoint;
        let is_live_session = self
            .entries
            .get(&device)
            .is_some_and(|entry| entry.participant.session == body.session);
        if is_live_session {
            return self.continue_session(beacon, now);
        }
        // A session we haven't heard: new, or replayed. A leave is sent
        // more than once, so a repeat of one changes nothing.
        if self.is_retired(&device, &body.session, now) {
            if body.state == BeaconState::Left {
                return Ok(Changed::default());
            }
            bail!("a beacon from a huddle session that already ended");
        }
        if let Some(entry) = self.entries.get(&device) {
            ensure!(
                body.sent_ms > entry.sent_ms,
                "a new session must be newer than the one it replaces"
            );
        }
        let BeaconState::Joined {
            camera,
            screen,
            reach,
        } = body.state
        else {
            // A leave that overtook its join: remembered, so the join is refused.
            self.retire(device, body.session, now);
            return Ok(Changed::default());
        };
        ensure!(!self.is_changing_too_often(&device, now), "joining too often");
        let mut changed = self.end(device, now);
        changed.channels.push(body.channel.clone());
        self.entries.insert(
            device,
            Entry {
                participant: Participant {
                    member: body.member,
                    endpoint: device,
                    channel: body.channel.clone(),
                    session: body.session,
                    camera,
                    screen,
                    reach,
                    media_wire: body.media_wire,
                },
                seq: body.seq,
                sent_ms: body.sent_ms,
                heard_at: now,
            },
        );
        self.last_change.insert(device, now);
        Ok(changed)
    }

    /// A refresh, a change of camera or screen, or a leave, in a session
    /// already heard.
    fn continue_session(&mut self, beacon: &Beacon, now: u64) -> Result<Changed> {
        let body = &beacon.body;
        let device = body.endpoint;
        let is_too_soon = self.is_changing_too_often(&device, now);
        let entry = self.entries.get_mut(&device).context("no live session")?;
        ensure!(body.seq > entry.seq, "a beacon older than one already heard");
        let BeaconState::Joined {
            camera,
            screen,
            reach,
        } = body.state
        else {
            return Ok(self.end(device, now));
        };
        entry.seq = body.seq;
        entry.sent_ms = body.sent_ms;
        entry.heard_at = now;
        let shown = &mut entry.participant;
        let is_visible_change = (shown.camera, shown.screen, shown.reach) != (camera, screen, reach);
        // Too soon after the last change, it still counts as a refresh; the
        // next refresh carries the change.
        if !is_visible_change || is_too_soon {
            return Ok(Changed::default());
        }
        (shown.camera, shown.screen, shown.reach) = (camera, screen, reach);
        let channel = shown.channel.clone();
        self.last_change.insert(device, now);
        Ok(Changed {
            channels: vec![channel],
        })
    }

    /// The devices in `channel`'s huddle, oldest first by member key.
    pub fn roster(&self, channel: &str, now: u64) -> Vec<Participant> {
        let mut roster: Vec<Participant> = self
            .entries
            .values()
            .filter(|entry| {
                entry.participant.channel == channel && now.saturating_sub(entry.heard_at) < EXPIRY_MS
            })
            .map(|entry| entry.participant.clone())
            .collect();
        roster.sort_by_key(|participant| *participant.endpoint.as_bytes());
        roster
    }

    /// Drops participants whose refreshes stopped. Returns their channels.
    pub fn forget_expired(&mut self, now: u64) -> Changed {
        let expired: Vec<PublicKey> = self
            .entries
            .iter()
            .filter(|(_, entry)| now.saturating_sub(entry.heard_at) >= EXPIRY_MS)
            .map(|(device, _)| *device)
            .collect();
        let mut changed = Changed::default();
        for device in expired {
            changed.channels.extend(self.end(device, now).channels);
        }
        for sessions in self.retired.values_mut() {
            sessions.retain(|(_, until)| *until > now);
        }
        self.retired.retain(|_, sessions| !sessions.is_empty());
        self.last_change
            .retain(|_, last| now.saturating_sub(*last) < MIN_CHANGE_GAP_MS);
        changed
    }

    /// Takes a device out of whatever huddle it was in, and retires its session.
    fn end(&mut self, device: PublicKey, now: u64) -> Changed {
        match self.entries.remove(&device) {
            Some(entry) => {
                self.retire(device, entry.participant.session, now);
                Changed {
                    channels: vec![entry.participant.channel],
                }
            }
            None => Changed::default(),
        }
    }

    fn retire(&mut self, device: PublicKey, session: SessionId, now: u64) {
        // A replay is possible for as long as its send time passes the skew check.
        let until = now + 2 * MAX_CLOCK_SKEW_MS as u64;
        let sessions = self.retired.entry(device).or_default();
        if sessions.len() >= MAX_RETIRED_PER_DEVICE {
            sessions.remove(0);
        }
        sessions.push((session, until));
    }

    fn is_retired(&self, device: &PublicKey, session: &SessionId, now: u64) -> bool {
        self.retired.get(device).is_some_and(|sessions| {
            sessions
                .iter()
                .any(|(retired, until)| retired == session && *until > now)
        })
    }

    fn is_changing_too_often(&self, device: &PublicKey, now: u64) -> bool {
        self.last_change
            .get(device)
            .is_some_and(|last| now.saturating_sub(*last) < MIN_CHANGE_GAP_MS)
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::huddle::beacon::{Beacon, Reach, tests::body};

    const WALL: i64 = 1_000_000_000;

    fn joined(identity: &SecretKey, channel: &str, session: SessionId, seq: u64, sent_ms: i64) -> Beacon {
        Beacon::sign(identity, "w1", body(identity, channel, session, seq, sent_ms)).unwrap()
    }

    fn left(identity: &SecretKey, channel: &str, session: SessionId, seq: u64, sent_ms: i64) -> Beacon {
        let mut leaving = body(identity, channel, session, seq, sent_ms);
        leaving.state = BeaconState::Left;
        Beacon::sign(identity, "w1", leaving).unwrap()
    }

    fn members_in(book: &HuddleBook, channel: &str, now: u64) -> Vec<PublicKey> {
        book.roster(channel, now)
            .into_iter()
            .map(|participant| participant.member)
            .collect()
    }

    #[test]
    fn a_join_shows_until_the_member_leaves() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        let changed = book
            .receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        assert_eq!(changed.channels, ["general"]);
        assert_eq!(members_in(&book, "general", 10), [ada.public()]);

        book.receive(&left(&ada, "general", [1; 16], 1, WALL + 5), WALL, 5_000)
            .unwrap();
        assert!(members_in(&book, "general", 5_001).is_empty());
    }

    #[test]
    fn refreshes_keep_a_participant_and_silence_expires_them() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        book.receive(
            &joined(&ada, "general", [1; 16], 1, WALL + 30_000),
            WALL + 30_000,
            30_000,
        )
        .unwrap();
        assert_eq!(members_in(&book, "general", 100_000), [ada.public()]);
        assert!(members_in(&book, "general", 30_000 + EXPIRY_MS).is_empty());
        assert_eq!(book.forget_expired(30_000 + EXPIRY_MS).channels, ["general"]);
    }

    #[test]
    fn a_clock_stepping_back_mid_call_changes_nothing() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        // The sender's clock jumped 40 s back; its counter still moves on.
        book.receive(&joined(&ada, "general", [1; 16], 1, WALL - 40_000), WALL, 30_000)
            .unwrap();
        assert_eq!(members_in(&book, "general", 100_000), [ada.public()]);
    }

    #[test]
    fn replays_are_refused() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        let first = joined(&ada, "general", [1; 16], 0, WALL);
        book.receive(&first, WALL, 0).unwrap();
        book.receive(&joined(&ada, "general", [1; 16], 1, WALL + 30_000), WALL, 30_000)
            .unwrap();
        assert!(
            book.receive(&first, WALL, 31_000).is_err(),
            "an older beacon of the same session"
        );

        book.receive(&left(&ada, "general", [1; 16], 2, WALL + 40_000), WALL, 40_000)
            .unwrap();
        assert!(
            book.receive(&first, WALL, 41_000).is_err(),
            "a join from a session that already ended"
        );
        assert!(members_in(&book, "general", 41_000).is_empty());
    }

    #[test]
    fn a_flood_of_leaves_stays_bounded() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        for index in 0..1_000u32 {
            let mut session = [0; 16];
            session[..4].copy_from_slice(&index.to_le_bytes());
            book.receive(&left(&ada, "general", session, 0, WALL), WALL, u64::from(index))
                .unwrap();
        }
        assert!(book.retired[&ada.public()].len() <= MAX_RETIRED_PER_DEVICE);
    }

    #[test]
    fn camera_and_screen_flapping_is_held_back() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        let mut camera_on = body(&ada, "general", [1; 16], 1, WALL);
        camera_on.state = BeaconState::Joined {
            camera: true,
            screen: false,
            reach: Reach::Direct,
        };
        let camera_on = Beacon::sign(&ada, "w1", camera_on).unwrap();
        assert_eq!(book.receive(&camera_on, WALL, 100).unwrap(), Changed::default());
        assert!(!book.roster("general", 100)[0].camera);
        let mut later = camera_on.body.clone();
        later.seq = 2;
        let later = Beacon::sign(&ada, "w1", later).unwrap();
        assert_eq!(book.receive(&later, WALL, 1_200).unwrap().channels, ["general"]);
        assert!(book.roster("general", 1_200)[0].camera);
    }

    #[test]
    fn change_records_are_forgotten() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        book.receive(&left(&ada, "general", [1; 16], 1, WALL), WALL, 10)
            .unwrap();
        book.forget_expired(10 + 2 * MAX_CLOCK_SKEW_MS as u64 + 1);
        assert!(book.last_change.is_empty());
    }

    #[test]
    fn a_repeated_leave_is_harmless() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        book.receive(&left(&ada, "general", [1; 16], 1, WALL), WALL, 1_000)
            .unwrap();
        let again = book
            .receive(&left(&ada, "general", [1; 16], 2, WALL), WALL, 1_001)
            .unwrap();
        assert_eq!(again, Changed::default());
    }

    #[test]
    fn a_beacon_outside_the_clock_skew_is_refused() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        assert!(
            book.receive(
                &joined(&ada, "general", [1; 16], 0, WALL - MAX_CLOCK_SKEW_MS - 1),
                WALL,
                0
            )
            .is_err()
        );
        assert!(
            book.receive(
                &joined(&ada, "general", [1; 16], 0, WALL + MAX_CLOCK_SKEW_MS + 1),
                WALL,
                0
            )
            .is_err()
        );
    }

    #[test]
    fn joining_another_huddle_leaves_the_first() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        let changed = book
            .receive(&joined(&ada, "design", [2; 16], 0, WALL + 5_000), WALL, 5_000)
            .unwrap();
        assert_eq!(changed.channels, ["general", "design"]);
        assert!(members_in(&book, "general", 5_000).is_empty());
        assert_eq!(members_in(&book, "design", 5_000), [ada.public()]);
    }

    #[test]
    fn a_new_session_must_be_newer_than_the_live_one() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        assert!(
            book.receive(&joined(&ada, "general", [2; 16], 0, WALL - 1), WALL, 5_000)
                .is_err()
        );
    }

    #[test]
    fn rapid_rejoining_is_refused() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        assert!(
            book.receive(&joined(&ada, "design", [2; 16], 0, WALL + 1), WALL, 200)
                .is_err()
        );
        book.receive(&joined(&ada, "design", [3; 16], 0, WALL + 2), WALL, 1_200)
            .unwrap();
    }

    #[test]
    fn retired_sessions_are_forgotten_once_a_replay_would_fail_anyway() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        book.receive(&left(&ada, "general", [1; 16], 1, WALL), WALL, 1_000)
            .unwrap();
        book.forget_expired(1_000 + 2 * MAX_CLOCK_SKEW_MS as u64 + 1);
        assert!(book.retired.is_empty());
    }
}
