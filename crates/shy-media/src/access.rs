//! Who may exchange audio with this device: the media node's policy. A peer
//! the core lists for the running call may read this call's own path and
//! publish only at its own; anyone else gets no session at all. Membership
//! is checked before this, when the connection opens (`ExtraProtocols`).

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use iroh::PublicKey;
use iroh_live::{
    Grant,
    moq_net::{Pattern, Patterns},
    node::policy::{self, Decision, PeerRequest, Policy, Rejection},
};

/// The running call's paths, shared with the node's policy.
#[derive(Debug, Clone, Default)]
pub(crate) struct Access(Arc<Mutex<State>>);

#[derive(Debug, Default)]
struct State {
    allowed: Option<Allowed>,
    /// Counts the calls opened, so an ended call's late update can't touch
    /// the next one.
    calls: u64,
}

/// One call's hold on the access: updates through it apply only while
/// that call is the running one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CallId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Allowed {
    pub(crate) my_path: String,
    /// Each listed peer and the path it publishes at.
    pub(crate) peers: BTreeMap<PublicKey, String>,
}

impl Access {
    pub(crate) fn open(&self, allowed: Allowed) -> CallId {
        let mut state = self.state();
        state.calls += 1;
        state.allowed = Some(allowed);
        CallId(state.calls)
    }

    pub(crate) fn set_peers(&self, call: CallId, peers: BTreeMap<PublicKey, String>) {
        let mut state = self.state();
        if CallId(state.calls) != call {
            return;
        }
        if let Some(allowed) = state.allowed.as_mut() {
            allowed.peers = peers;
        }
    }

    /// Ends the call: nobody may read or publish anything.
    pub(crate) fn close(&self) {
        self.state().allowed = None;
    }

    pub(crate) fn policy(&self) -> impl Policy<PublicKey> + use<> {
        let access = self.clone();
        policy::from_fn(move |request: &PeerRequest| access.decide(request.peer()))
    }

    fn decide(&self, peer: &PublicKey) -> Decision {
        match self
            .state()
            .allowed
            .as_ref()
            .and_then(|allowed| grant(allowed, peer))
        {
            Some(grant) => Ok(grant.into()),
            None => Err(Rejection::new("not in this huddle")),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .expect("the huddle access lock is never held across a panic")
    }
}

/// `None` refuses the peer. A path a pattern can't spell refuses it too:
/// granting a wider pattern instead would open more than one path.
fn grant(allowed: &Allowed, peer: &PublicKey) -> Option<Grant> {
    let their_path = allowed.peers.get(peer)?;
    Some(Grant {
        subscribe: Patterns::from(Pattern::literal(&allowed.my_path).ok()?),
        publish: Patterns::from(Pattern::literal(their_path).ok()?),
    })
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    fn peer(seed: u8) -> PublicKey {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn call_with(listed: &[(u8, &str)]) -> Allowed {
        Allowed {
            my_path: "h-mine".into(),
            peers: listed
                .iter()
                .map(|(seed, path)| (peer(*seed), (*path).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn a_listed_peer_may_read_only_this_calls_path_and_publish_only_its_own() {
        let grant = grant(&call_with(&[(1, "h-theirs")]), &peer(1)).expect("listed");
        assert!(grant.allows_subscribe("h-mine"));
        assert!(!grant.allows_subscribe("h-other"), "a guessed path");
        assert!(!grant.allows_subscribe("h-mine/more"), "below the path");
        assert!(grant.allows_publish("h-theirs"));
        assert!(!grant.allows_publish("h-mine"), "standing in for this device");
        assert!(
            !grant.allows_publish("h-someone-else"),
            "standing in for another peer"
        );
    }

    #[test]
    fn a_peer_who_isnt_listed_gets_nothing() {
        assert_eq!(grant(&call_with(&[(1, "h-theirs")]), &peer(2)), None);
    }

    #[test]
    fn a_path_no_pattern_can_spell_is_refused() {
        assert_eq!(grant(&call_with(&[(1, "h-*")]), &peer(1)), None);
        let mut call = call_with(&[(1, "h-theirs")]);
        call.my_path = "*".into();
        assert_eq!(grant(&call, &peer(1)), None);
    }

    #[test]
    fn without_a_call_everyone_is_refused() {
        let access = Access::default();
        assert!(access.decide(&peer(1)).is_err());
        access.open(call_with(&[(1, "h-theirs")]));
        assert!(access.decide(&peer(1)).is_ok());
        access.close();
        assert!(access.decide(&peer(1)).is_err());
    }

    #[test]
    fn a_peer_taken_off_the_list_is_refused() {
        let access = Access::default();
        let call = access.open(call_with(&[(1, "h-theirs")]));
        access.set_peers(call, BTreeMap::new());
        assert!(access.decide(&peer(1)).is_err());
    }

    #[test]
    fn an_ended_calls_late_update_cant_open_the_next_call() {
        let access = Access::default();
        let ended = access.open(call_with(&[(1, "h-theirs")]));
        access.close();
        access.open(call_with(&[(2, "h-next")]));
        access.set_peers(ended, call_with(&[(1, "h-theirs")]).peers);
        assert!(
            access.decide(&peer(1)).is_err(),
            "the old call's peer got into the new one"
        );
        assert!(access.decide(&peer(2)).is_ok());
    }
}
