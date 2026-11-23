//! The ring for an incoming DM huddle on this device: it starts with the
//! call and stops when this member answers or declines, when the caller
//! leaves, or when nobody answers in time. One ring at a time.

use iroh::PublicKey;

use super::Workspace;
use crate::{
    huddle::{
        media::Ringing,
        ring::{IncomingHuddle, RING_MS, ring_ends, rings},
    },
    views::{self, Change, member_id},
};

pub(crate) struct Ring {
    channel: String,
    caller: PublicKey,
    caller_name: String,
    started_ms: u64,
    _sound: Ringing,
}

impl Ring {
    fn view(&self) -> views::IncomingHuddle {
        views::IncomingHuddle {
            channel: self.channel.clone(),
            member: member_id(&self.caller),
            member_name: self.caller_name.clone(),
        }
    }
}

impl Workspace {
    /// Rings for `caller`'s new huddle in `channel`, if `incoming` should.
    /// A ring already sounding is replaced.
    pub(super) fn ring_for(
        &self,
        channel: &str,
        caller: &PublicKey,
        caller_name: &str,
        incoming: IncomingHuddle,
    ) {
        let Some(media) = &self.inner.huddle_media else {
            return;
        };
        if !rings(incoming) {
            return;
        }
        let sound = media.ring();
        let (replaced, ringing) = {
            let mut huddles = self.huddles();
            let started_ms = huddles.now();
            let ring = Ring {
                channel: channel.to_owned(),
                caller: *caller,
                caller_name: caller_name.to_owned(),
                started_ms,
                _sound: sound,
            };
            let ringing = ring.view();
            (huddles.ringing.replace(ring), ringing)
        };
        if let Some(replaced) = replaced {
            self.end_ring(replaced);
        }
        self.emit(Change::HuddleRinging {
            channel: ringing.channel,
            member: ringing.member,
            member_name: ringing.member_name,
        });
    }

    /// The incoming huddle ringing now, if any: for a window that opens or
    /// reloads mid-ring.
    pub fn huddle_ringing(&self) -> Option<views::IncomingHuddle> {
        self.huddles().ringing.as_ref().map(Ring::view)
    }

    /// Stops the ring: this member doesn't want to take the call.
    pub fn decline_huddle(&self) {
        self.stop_ring();
    }

    pub(super) fn stop_ring(&self) {
        let ring = self.huddles().ringing.take();
        if let Some(ring) = ring {
            self.end_ring(ring);
        }
    }

    /// Stops the ring if its caller left a huddle in `changed` (or this
    /// member joined it elsewhere), or if nobody answered in time.
    pub(super) fn stop_ring_if_over(&self, changed: &[String]) {
        let me = self.me();
        let over = {
            let mut huddles = self.huddles();
            let now = huddles.now();
            let is_over = huddles.ringing.as_ref().is_some_and(|ring| {
                let is_unanswered = now.saturating_sub(ring.started_ms) >= RING_MS;
                let has_changed = changed.contains(&ring.channel);
                is_unanswered || (has_changed && ring_ends(&ring.caller, &me, &huddles.roster(&ring.channel)))
            });
            if is_over { huddles.ringing.take() } else { None }
        };
        if let Some(ring) = over {
            self.end_ring(ring);
        }
    }

    /// Silences `ring` outside the huddle lock, and says so.
    fn end_ring(&self, ring: Ring) {
        let channel = ring.channel.clone();
        drop(ring);
        self.emit(Change::HuddleRingStopped { channel });
    }
}
