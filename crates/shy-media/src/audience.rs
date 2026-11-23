//! Following the core's list of peers: whom to start and stop playing as it
//! changes. The list is the whole rule; nobody else is offered or played.

use std::collections::BTreeMap;

use iroh::PublicKey;

/// What to do so the peers played match the wanted ones.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PeerChanges {
    pub(crate) stop: Vec<PublicKey>,
    /// Each peer and the path to play.
    pub(crate) start: Vec<(PublicKey, String)>,
}

/// A peer whose path changed rejoined: its old subscription stops and the
/// new path starts.
pub(crate) fn peer_changes(
    playing: &BTreeMap<PublicKey, String>,
    wanted: &BTreeMap<PublicKey, String>,
) -> PeerChanges {
    let stop = playing
        .iter()
        .filter(|(peer, path)| wanted.get(*peer) != Some(*path))
        .map(|(peer, _)| *peer)
        .collect();
    let start = wanted
        .iter()
        .filter(|(peer, path)| playing.get(*peer) != Some(*path))
        .map(|(peer, path)| (*peer, path.clone()))
        .collect();
    PeerChanges { stop, start }
}

/// The listed peers that can be told apart by path. Paths are random per
/// join, so a path that's this device's own, or that two peers share, means
/// someone copied another's session from its beacon: no one at that path is
/// played or let in, since there's no telling who is genuine.
pub(crate) fn distinct_peers(
    my_path: &str,
    listed: &BTreeMap<PublicKey, String>,
) -> BTreeMap<PublicKey, String> {
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for path in listed.values() {
        *uses.entry(path.as_str()).or_default() += 1;
    }
    listed
        .iter()
        .filter(|(_, path)| path.as_str() != my_path && uses[path.as_str()] == 1)
        .map(|(peer, path)| (*peer, path.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    fn peer(seed: u8) -> PublicKey {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    fn peers(entries: &[(u8, &str)]) -> BTreeMap<PublicKey, String> {
        entries
            .iter()
            .map(|(seed, path)| (peer(*seed), (*path).to_owned()))
            .collect()
    }

    #[test]
    fn newcomers_start_and_leavers_stop() {
        let changes = peer_changes(&peers(&[(1, "a"), (2, "b")]), &peers(&[(2, "b"), (3, "c")]));
        assert_eq!(
            changes,
            PeerChanges {
                stop: vec![peer(1)],
                start: vec![(peer(3), "c".into())],
            }
        );
    }

    #[test]
    fn a_rejoin_at_a_new_path_restarts_the_peer() {
        let changes = peer_changes(&peers(&[(1, "old")]), &peers(&[(1, "new")]));
        assert_eq!(
            changes,
            PeerChanges {
                stop: vec![peer(1)],
                start: vec![(peer(1), "new".into())],
            }
        );
    }

    #[test]
    fn nothing_changes_when_the_peers_are_the_same() {
        let same = peers(&[(1, "a"), (2, "b")]);
        assert_eq!(peer_changes(&same, &same), PeerChanges::default());
    }

    #[test]
    fn everyone_stops_when_the_list_empties() {
        let changes = peer_changes(&peers(&[(1, "a"), (2, "b")]), &BTreeMap::new());
        assert_eq!(changes.stop.len(), 2);
        assert!(changes.start.is_empty());
    }

    #[test]
    fn peers_sharing_a_path_are_all_left_out() {
        let listed = peers(&[(1, "a"), (2, "a"), (3, "c")]);
        assert_eq!(distinct_peers("mine", &listed), peers(&[(3, "c")]));
    }

    #[test]
    fn a_peer_claiming_this_devices_own_path_is_left_out() {
        let listed = peers(&[(1, "mine"), (2, "b")]);
        assert_eq!(distinct_peers("mine", &listed), peers(&[(2, "b")]));
    }
}
