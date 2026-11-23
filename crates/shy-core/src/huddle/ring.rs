//! When an incoming huddle rings this device, and when the ring stops, and
//! how often the join and leave sounds may play.

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
}
