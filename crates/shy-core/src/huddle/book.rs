//! Who is in which huddle, from verified beacons, in memory only. A device's
//! beacons are ordered within its session by `seq`, so a clock that steps
//! backwards mid-call changes nothing; a new session must fall within the
//! clock skew and can't be one that already ended. Entries expire when their
//! refreshes stop (a crash, a lost network); the session isn't ended, so a
//! later refresh brings the participant back.

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
/// How long an expired session is kept, hidden, for its refreshes to bring
/// back. Past it, every beacon heard before fails the clock skew check, so
/// forgetting the session can't let one be replayed.
const LAPSED_KEPT_MS: u64 = 2 * MAX_CLOCK_SKEW_MS as u64;
/// Visible changes (joining, camera, screen) one device may make in a quick
/// burst, like a misclick and the right huddle. Beyond that, one more is
/// earned per `CHANGE_EARNED_MS`, so nobody can make everyone's roster
/// redraw nonstop.
const CHANGE_BURST: u32 = 4;
const CHANGE_EARNED_MS: u64 = 1000;
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

#[derive(Clone, Copy)]
struct Allowance {
    left: u32,
    earned_until: u64,
}

struct Entry {
    participant: Participant,
    seq: u64,
    sent_ms: i64,
    heard_at: u64,
    /// Expired, and reported so: hidden until a refresh brings it back.
    is_lapsed: bool,
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
    allowances: HashMap<PublicKey, Allowance>,
}

impl HuddleBook {
    /// Takes a beacon that was already verified and whose sender may be in
    /// its channel's huddle. `wall_ms` is our clock, `now` a monotonic one.
    /// Call `forget_expired` first, so the expiry of a silent participant is
    /// reported even if this beacon is refused.
    pub fn receive(&mut self, beacon: &Beacon, wall_ms: i64, now: u64) -> Result<Changed> {
        ensure!(
            beacon.body.sent_ms.abs_diff(wall_ms) <= MAX_CLOCK_SKEW_MS.unsigned_abs(),
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
        ensure!(self.spend_change(device, now), "joining too often");
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
                is_lapsed: false,
            },
        );
        Ok(changed)
    }

    /// A refresh, a change of camera or screen, or a leave, in a session
    /// already heard.
    fn continue_session(&mut self, beacon: &Beacon, now: u64) -> Result<Changed> {
        let body = &beacon.body;
        let device = body.endpoint;
        let entry = self.entries.get(&device).context("no live session")?;
        ensure!(body.seq > entry.seq, "a beacon older than one already heard");
        let BeaconState::Joined {
            camera,
            screen,
            reach,
        } = body.state
        else {
            return Ok(self.end(device, now));
        };
        let is_back = entry.is_lapsed;
        let shown = &entry.participant;
        let is_visible_change = (shown.camera, shown.screen, shown.reach) != (camera, screen, reach);
        // Over the allowance, it still counts as a refresh; a later refresh
        // carries the change.
        let is_shown = is_back || (is_visible_change && self.spend_change(device, now));
        let entry = self.entries.get_mut(&device).context("no live session")?;
        entry.seq = body.seq;
        entry.sent_ms = body.sent_ms;
        entry.heard_at = now;
        entry.is_lapsed = false;
        if !is_shown {
            return Ok(Changed::default());
        }
        let shown = &mut entry.participant;
        (shown.camera, shown.screen, shown.reach) = (camera, screen, reach);
        Ok(Changed {
            channels: vec![shown.channel.clone()],
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

    /// Hides participants whose refreshes stopped, and forgets them once no
    /// refresh can bring them back. Returns the channels of those newly hidden.
    pub fn forget_expired(&mut self, now: u64) -> Changed {
        let mut changed = Changed::default();
        self.entries.retain(|_, entry| {
            let silent_for = now.saturating_sub(entry.heard_at);
            if silent_for >= EXPIRY_MS && !entry.is_lapsed {
                entry.is_lapsed = true;
                changed.channels.push(entry.participant.channel.clone());
            }
            silent_for < LAPSED_KEPT_MS
        });
        for sessions in self.retired.values_mut() {
            sessions.retain(|(_, until)| *until > now);
        }
        self.retired.retain(|_, sessions| !sessions.is_empty());
        // A device whose allowance is full again needs no record.
        let refill_ms = u64::from(CHANGE_BURST) * CHANGE_EARNED_MS;
        self.allowances
            .retain(|_, allowance| now.saturating_sub(allowance.earned_until) < refill_ms);
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

    /// Uses one of the device's visible changes, if it has any left.
    fn spend_change(&mut self, device: PublicKey, now: u64) -> bool {
        let allowance = self.allowances.entry(device).or_insert(Allowance {
            left: CHANGE_BURST,
            earned_until: now,
        });
        let earned = now.saturating_sub(allowance.earned_until) / CHANGE_EARNED_MS;
        if earned > 0 {
            let earned_changes = u32::try_from(earned).unwrap_or(u32::MAX);
            allowance.left = CHANGE_BURST.min(allowance.left.saturating_add(earned_changes));
            allowance.earned_until += earned * CHANGE_EARNED_MS;
        }
        if allowance.left == 0 {
            return false;
        }
        allowance.left -= 1;
        true
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
    fn comes_back_on_the_next_refresh_after_expiring() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        assert_eq!(book.forget_expired(EXPIRY_MS).channels, ["general"]);
        assert!(members_in(&book, "general", EXPIRY_MS).is_empty());

        let back = EXPIRY_MS + 10_000;
        let changed = book
            .receive(
                &joined(&ada, "general", [1; 16], 3, WALL + back as i64),
                WALL + back as i64,
                back,
            )
            .unwrap();
        assert_eq!(changed.channels, ["general"]);
        assert_eq!(members_in(&book, "general", back), [ada.public()]);
    }

    #[test]
    fn an_expiry_is_reported_once() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        assert_eq!(book.forget_expired(EXPIRY_MS).channels, ["general"]);
        assert!(book.forget_expired(EXPIRY_MS + 1_000).channels.is_empty());
    }

    #[test]
    fn an_old_beacon_cant_bring_back_an_expired_participant() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        let first = joined(&ada, "general", [1; 16], 0, WALL);
        book.receive(&first, WALL, 0).unwrap();
        book.receive(
            &joined(&ada, "general", [1; 16], 1, WALL + 30_000),
            WALL + 30_000,
            30_000,
        )
        .unwrap();
        let lapsed = 30_000 + EXPIRY_MS;
        book.forget_expired(lapsed);

        assert!(book.receive(&first, WALL + 60_000, lapsed).is_err());
        assert!(members_in(&book, "general", lapsed).is_empty());
    }

    #[test]
    fn an_expired_session_is_forgotten_but_can_still_come_back() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        book.receive(&joined(&ada, "general", [1; 16], 0, WALL), WALL, 0)
            .unwrap();
        let gone = 2 * MAX_CLOCK_SKEW_MS as u64;
        book.forget_expired(EXPIRY_MS);
        book.forget_expired(gone);
        assert!(book.entries.is_empty());

        let back = gone + 1_000;
        book.receive(
            &joined(&ada, "general", [1; 16], 9, WALL + back as i64),
            WALL + back as i64,
            back,
        )
        .unwrap();
        assert_eq!(members_in(&book, "general", back), [ada.public()]);
    }

    #[test]
    fn a_send_time_at_the_edge_of_the_clock_is_refused_without_panicking() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        assert!(
            book.receive(&joined(&ada, "general", [1; 16], 0, i64::MIN), WALL, 0)
                .is_err()
        );
        assert!(
            book.receive(&joined(&ada, "general", [2; 16], 0, i64::MAX), -WALL, 0)
                .is_err()
        );
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
        let toggle = |seq: u64| {
            let mut changed = body(&ada, "general", [1; 16], seq, WALL);
            changed.state = BeaconState::Joined {
                camera: seq % 2 == 1,
                screen: false,
                reach: Reach::Direct,
            };
            Beacon::sign(&ada, "w1", changed).unwrap()
        };
        // The join used one change; the rest of the burst shows.
        for seq in 1..u64::from(CHANGE_BURST) {
            assert_eq!(
                book.receive(&toggle(seq), WALL, 10 + seq).unwrap().channels,
                ["general"]
            );
        }
        let held = toggle(u64::from(CHANGE_BURST));
        assert_eq!(book.receive(&held, WALL, 20).unwrap(), Changed::default());
        let shown_camera = book.roster("general", 20)[0].camera;
        let later = toggle(u64::from(CHANGE_BURST) + 2);
        assert_eq!(
            book.receive(&later, WALL, 20 + CHANGE_EARNED_MS)
                .unwrap()
                .channels,
            ["general"]
        );
        assert_ne!(
            book.roster("general", 20 + CHANGE_EARNED_MS)[0].camera,
            shown_camera
        );
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
        assert!(book.allowances.is_empty());
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
    fn a_quick_switch_is_fine_but_a_flood_is_refused() {
        let ada = SecretKey::generate();
        let mut book = HuddleBook::default();
        for index in 0..CHANGE_BURST {
            let session = [u8::try_from(index).unwrap() + 1; 16];
            let sent_ms = WALL + i64::from(index);
            book.receive(
                &joined(&ada, "general", session, 0, sent_ms),
                WALL,
                u64::from(index),
            )
            .unwrap();
        }
        let flood = joined(&ada, "design", [99; 16], 0, WALL + 100);
        assert!(book.receive(&flood, WALL, 50).is_err());
        let later = joined(&ada, "design", [100; 16], 0, WALL + 200);
        book.receive(&later, WALL, 50 + CHANGE_EARNED_MS).unwrap();
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
