//! Presence and status. Presence (online, away) travels only as signed
//! heartbeats over gossip and is never stored; status is a synced op.

use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::{
    ops::{Op, StatusAlerts, is_valid_status, now_ms},
    views::{PresenceState, StatusUpdate},
};

/// A heartbeat newer or older than this, by the sender's clock, is refused,
/// so a recorded one can't be replayed later.
const MAX_CLOCK_SKEW_MS: i64 = 2 * 60 * 1000;
/// Without a heartbeat for this long, a member counts as unreachable.
pub(crate) const PRESENCE_TIMEOUT_MS: i64 = 75 * 1000;
/// Presence changes from one member closer together than this are ignored,
/// so nobody can make everyone's member list redraw nonstop.
const MIN_CHANGE_GAP_MS: i64 = 1000;
/// Peers reconnecting over and over each ask for a heartbeat; one this soon
/// after the last, saying the same, is skipped.
const MIN_SEND_GAP_MS: i64 = 5000;
const SIGNATURE_DOMAIN: &str = "shyspace presence v1";
const APPEAR_OFFLINE_KEY: &str = "appear_offline";

/// "I'm here", signed by the member, for this workspace only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub member: PublicKey,
    pub state: PresenceState,
    pub sent_ms: i64,
    pub signature: Vec<u8>,
}

struct Heard {
    state: PresenceState,
    sent_ms: i64,
    received_ms: i64,
}

/// Who was last heard from, and when (by our clock).
#[derive(Default)]
pub(crate) struct PresenceBook {
    heard: HashMap<PublicKey, Heard>,
}

impl PresenceBook {
    /// Unreachable without a recent heartbeat.
    fn state_of(&self, member: &PublicKey, now_ms: i64) -> PresenceState {
        match self.heard.get(member) {
            Some(heard) if now_ms - heard.received_ms < PRESENCE_TIMEOUT_MS => heard.state,
            _ => PresenceState::Unreachable,
        }
    }

    /// Newer heartbeats are accepted, but a change of state at most once per
    /// `MIN_CHANGE_GAP_MS`; repeats of the same state change nothing visible.
    fn should_accept(&self, heartbeat: &Heartbeat, now_ms: i64) -> bool {
        self.heard.get(&heartbeat.member).is_none_or(|heard| {
            let is_change = heartbeat.state != heard.state;
            heartbeat.sent_ms > heard.sent_ms
                && (!is_change || now_ms - heard.received_ms >= MIN_CHANGE_GAP_MS)
        })
    }
}

/// What the heartbeat loop should do next.
pub(crate) enum NextBeat {
    Now(Heartbeat),
    /// The same beat went out moments ago; send it after this.
    After(Duration),
    /// Appearing offline: nothing to send.
    Silent,
}

/// What this device says about itself. `appear_offline` is kept in the
/// database, so it holds from the first moment after a restart.
#[derive(Default)]
pub(crate) struct OwnPresence {
    pub is_away: bool,
    pub appear_offline: bool,
    /// The last heartbeat's state and when it went out.
    pub last_sent: Option<(PresenceState, i64)>,
}

fn signed_bytes(
    workspace_id: &str,
    member: &PublicKey,
    state: PresenceState,
    sent_ms: i64,
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        SIGNATURE_DOMAIN,
        workspace_id,
        member.as_bytes(),
        state,
        sent_ms,
    ))?)
}

pub(crate) fn load_appear_offline(store: &crate::storage::Store) -> Result<bool> {
    Ok(store.meta(APPEAR_OFFLINE_KEY)?.is_some_and(|value| value == [1]))
}

impl Workspace {
    pub fn set_status(&self, status: StatusUpdate<'_>) -> Result<()> {
        let (emoji, text) = (status.emoji.trim(), status.text.trim());
        ensure!(
            is_valid_status(emoji, text),
            "a status has up to 16 characters of emoji and 100 of plain text"
        );
        self.write(Op::Status {
            emoji: emoji.to_owned(),
            text: text.to_owned(),
            until_ms: status.until_ms,
            alerts: status.alerts.into(),
        })
    }

    pub fn clear_status(&self) -> Result<()> {
        self.write(Op::Status {
            emoji: String::new(),
            text: String::new(),
            until_ms: None,
            alerts: StatusAlerts::All,
        })
    }

    /// Away after a while without input; tells others at once.
    pub fn set_away(&self, is_away: bool) {
        let changed = {
            let mut own = self.own();
            let changed = own.is_away != is_away;
            own.is_away = is_away;
            changed
        };
        if changed {
            self.inner.heartbeat_now.notify_one();
        }
    }

    pub fn appear_offline(&self) -> Result<bool> {
        Ok(self.own().appear_offline)
    }

    /// Stops (or resumes) heartbeats; remembered across restarts.
    pub fn set_appear_offline(&self, appear_offline: bool) -> Result<()> {
        self.state()
            .store
            .set_meta(APPEAR_OFFLINE_KEY, &[u8::from(appear_offline)])
            .context("saving whether to appear offline")?;
        self.own().appear_offline = appear_offline;
        self.inner.heartbeat_now.notify_one();
        Ok(())
    }

    fn own(&self) -> std::sync::MutexGuard<'_, OwnPresence> {
        self.inner
            .own_presence
            .lock()
            .expect("presence lock poisoned by an earlier panic")
    }

    fn own_state(&self) -> PresenceState {
        let own = self.own();
        if own.appear_offline {
            PresenceState::Unreachable
        } else if own.is_away {
            PresenceState::Away
        } else {
            PresenceState::Online
        }
    }

    /// The heartbeat to send now: none when appearing offline, or a wait when
    /// the same one went out moments ago.
    pub(crate) fn next_heartbeat(&self) -> Result<NextBeat> {
        let state = self.own_state();
        if state == PresenceState::Unreachable {
            return Ok(NextBeat::Silent);
        }
        let now = now_ms()?;
        {
            let mut own = self.own();
            if let Some((sent_state, sent_ms)) = own.last_sent
                && sent_state == state
                && now - sent_ms < MIN_SEND_GAP_MS
            {
                let wait_ms = MIN_SEND_GAP_MS - (now - sent_ms);
                return Ok(NextBeat::After(Duration::from_millis(wait_ms.unsigned_abs())));
            }
            own.last_sent = Some((state, now));
        }
        self.heartbeat_at(state, now).map(NextBeat::Now)
    }

    pub fn heartbeat(&self, state: PresenceState) -> Result<Heartbeat> {
        self.heartbeat_at(state, now_ms()?)
    }

    pub(crate) fn heartbeat_at(&self, state: PresenceState, sent_ms: i64) -> Result<Heartbeat> {
        let member = self.me();
        let bytes = signed_bytes(&self.inner.config.workspace_id, &member, state, sent_ms)?;
        Ok(Heartbeat {
            member,
            state,
            sent_ms,
            signature: self.inner.identity.sign(&bytes).to_bytes().to_vec(),
        })
    }

    /// Accepts a heartbeat signed by a current member within the clock skew.
    /// Returns whether that member's presence changed.
    pub fn receive_heartbeat(&self, heartbeat: &Heartbeat) -> Result<bool> {
        let now = now_ms()?;
        ensure!(
            heartbeat.sent_ms.abs_diff(now) <= MAX_CLOCK_SKEW_MS.unsigned_abs(),
            "heartbeat too old or from the future"
        );
        ensure!(
            self.current_members()?.contains(&heartbeat.member),
            "heartbeat from someone who isn't a member"
        );
        let signature: [u8; 64] = heartbeat
            .signature
            .as_slice()
            .try_into()
            .context("malformed heartbeat signature")?;
        let bytes = signed_bytes(
            &self.inner.config.workspace_id,
            &heartbeat.member,
            heartbeat.state,
            heartbeat.sent_ms,
        )?;
        heartbeat
            .member
            .verify(&bytes, &Signature::from_bytes(&signature))
            .context("bad heartbeat signature")?;
        let mut book = self
            .inner
            .presence
            .lock()
            .expect("presence lock poisoned by an earlier panic");
        let before = book.state_of(&heartbeat.member, now);
        if book.should_accept(heartbeat, now) {
            book.heard.insert(
                heartbeat.member,
                Heard {
                    state: heartbeat.state,
                    sent_ms: heartbeat.sent_ms,
                    received_ms: now,
                },
            );
        }
        Ok(book.state_of(&heartbeat.member, now) != before)
    }

    pub(crate) fn presence_of(&self, member: &PublicKey, now_ms: i64) -> PresenceState {
        if *member == self.me() {
            return self.own_state();
        }
        self.inner
            .presence
            .lock()
            .expect("presence lock poisoned by an earlier panic")
            .state_of(member, now_ms)
    }
}
