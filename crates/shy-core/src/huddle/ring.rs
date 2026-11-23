//! When an incoming huddle rings this device, and when the ring stops, and
//! how often the join and leave sounds may play.

use std::collections::HashMap;

use iroh::PublicKey;

use super::{book::Participant, media::Cue};

/// How long a ring lasts if nobody answers.
pub const RING_MS: u64 = 30 * 1000;
/// Others coming and going within this make one sound, not a burst.
pub const CUE_GAP_MS: u64 = 2 * 1000;

/// What's known when someone else starts a huddle.
#[derive(Debug, Clone, Copy)]
pub struct IncomingHuddle {
    pub is_dm: bool,
    /// The caller's first beacon in its session. Beacons are never caught up
    /// later and a refresh comes every 30 s, the length of a ring, so a later
    /// one means the call started long enough ago that it shouldn't ring.
    pub is_first_beacon: bool,
    /// This device is in a huddle already: the call shows, without a sound.
    pub am_in_a_huddle: bool,
    /// The DM's notification level and this member's status allow alerts.
    pub may_alert: bool,
}

pub fn rings(incoming: IncomingHuddle) -> bool {
    incoming.is_dm && incoming.is_first_beacon && !incoming.am_in_a_huddle && incoming.may_alert
}

/// Whether a ring from `caller` stops, now that its huddle is `roster`:
/// the caller left, or this member joined from another device.
pub fn ring_ends(caller: &PublicKey, me: &PublicKey, roster: &[Participant]) -> bool {
    let has = |member: &PublicKey| roster.iter().any(|participant| participant.member == *member);
    !has(caller) || has(me)
}

/// Lets others' sounds play at most once per [`CUE_GAP_MS`]. This device's
/// own joining and leaving always sound, and start the gap.
#[derive(Debug, Default)]
pub struct CueLimit {
    last_ms: Option<u64>,
}

impl CueLimit {
    /// Whether `cue` plays at `now_ms` (a monotonic clock).
    pub fn allows(&mut self, cue: Cue, now_ms: u64) -> bool {
        let is_own = matches!(cue, Cue::Joined | Cue::Left);
        let is_spaced = self
            .last_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= CUE_GAP_MS);
        if !is_own && !is_spaced {
            return false;
        }
        self.last_ms = Some(now_ms);
        true
    }
}

/// How many calls from one caller may alert this member within
/// [`CALLER_ALERT_WINDOW_MS`]: enough for a redial after a dropped call,
/// not for ringing someone nonstop.
pub const CALLER_ALERTS: usize = 3;
pub const CALLER_ALERT_WINDOW_MS: u64 = 2 * 60 * 1000;

/// The recent alerts per caller, against [`CALLER_ALERTS`]. This member
/// joining a caller's call forgets that caller's.
#[derive(Debug, Default)]
pub struct CallerAlerts {
    recent_ms: HashMap<PublicKey, Vec<u64>>,
}

impl CallerAlerts {
    /// Whether a call from `caller` at `now_ms` (a monotonic clock) may alert.
    pub fn may_alert(&self, caller: &PublicKey, now_ms: u64) -> bool {
        self.recent_ms
            .get(caller)
            .is_none_or(|alerts| alerts.iter().filter(|at| is_recent(**at, now_ms)).count() < CALLER_ALERTS)
    }

    pub fn alerted(&mut self, caller: PublicKey, now_ms: u64) {
        for alerts in self.recent_ms.values_mut() {
            alerts.retain(|at| is_recent(*at, now_ms));
        }
        self.recent_ms.retain(|_, alerts| !alerts.is_empty());
        self.recent_ms.entry(caller).or_default().push(now_ms);
    }

    pub fn answered(&mut self, caller: &PublicKey) {
        self.recent_ms.remove(caller);
    }
}

fn is_recent(alerted_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(alerted_ms) < CALLER_ALERT_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::huddle::beacon::Reach;

    fn calling() -> IncomingHuddle {
        IncomingHuddle {
            is_dm: true,
            is_first_beacon: true,
            am_in_a_huddle: false,
            may_alert: true,
        }
    }

    #[test]
    fn a_new_dm_call_rings() {
        assert!(rings(calling()));
    }

    #[test]
    fn nothing_else_rings() {
        let not_a_dm = IncomingHuddle {
            is_dm: false,
            ..calling()
        };
        let ongoing = IncomingHuddle {
            is_first_beacon: false,
            ..calling()
        };
        let busy = IncomingHuddle {
            am_in_a_huddle: true,
            ..calling()
        };
        let quiet = IncomingHuddle {
            may_alert: false,
            ..calling()
        };
        for incoming in [not_a_dm, ongoing, busy, quiet] {
            assert!(!rings(incoming), "{incoming:?}");
        }
    }

    fn participant(member: &PublicKey) -> Participant {
        Participant {
            member: *member,
            endpoint: *member,
            channel: "dm".into(),
            session: [1; 16],
            camera: false,
            screen: false,
            reach: Reach::Direct,
            media_wire: 1,
        }
    }

    #[test]
    fn the_ring_stops_when_the_caller_goes_or_you_join() {
        let [caller, me, other] = [(); 3].map(|()| SecretKey::generate().public());
        assert!(!ring_ends(&caller, &me, &[participant(&caller)]));
        assert!(!ring_ends(
            &caller,
            &me,
            &[participant(&caller), participant(&other)]
        ));
        assert!(ring_ends(&caller, &me, &[]), "the huddle emptied");
        assert!(ring_ends(&caller, &me, &[participant(&other)]), "the caller left");
        assert!(
            ring_ends(&caller, &me, &[participant(&caller), participant(&me)]),
            "you joined from another device"
        );
    }

    #[test]
    fn others_coming_and_going_together_make_one_sound() {
        let mut limit = CueLimit::default();
        assert!(limit.allows(Cue::PeerJoined, 10_000));
        assert!(!limit.allows(Cue::PeerJoined, 10_500));
        assert!(!limit.allows(Cue::PeerLeft, 11_900), "dropped and rejoined");
        assert!(limit.allows(Cue::PeerLeft, 12_000));
    }

    #[test]
    fn your_own_coming_and_going_always_sound() {
        let mut limit = CueLimit::default();
        assert!(limit.allows(Cue::PeerJoined, 10_000));
        assert!(limit.allows(Cue::Left, 10_100));
        assert!(limit.allows(Cue::Joined, 10_200));
        assert!(!limit.allows(Cue::PeerJoined, 11_000), "right after your own");
    }

    #[test]
    fn a_caller_may_alert_a_few_times_then_waits() {
        let ada = SecretKey::generate().public();
        let bob = SecretKey::generate().public();
        let mut alerts = CallerAlerts::default();
        for call in 0..CALLER_ALERTS as u64 {
            assert!(alerts.may_alert(&ada, call));
            alerts.alerted(ada, call);
        }
        assert!(!alerts.may_alert(&ada, CALLER_ALERTS as u64));
        assert!(alerts.may_alert(&bob, 1), "others still alert");
        assert!(
            alerts.may_alert(&ada, CALLER_ALERT_WINDOW_MS),
            "the oldest alert has aged out"
        );

        alerts.answered(&ada);
        assert!(
            alerts.may_alert(&ada, CALLER_ALERTS as u64),
            "a call taken starts afresh"
        );
    }

    #[test]
    fn old_alerts_are_forgotten() {
        let mut alerts = CallerAlerts::default();
        for _ in 0..100 {
            alerts.alerted(SecretKey::generate().public(), 0);
        }
        alerts.alerted(SecretKey::generate().public(), CALLER_ALERT_WINDOW_MS);
        assert_eq!(alerts.recent_ms.len(), 1);
    }
}
