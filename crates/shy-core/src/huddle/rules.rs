//! Who may be in a channel's huddle, and whom a device exchanges audio with.
//! The one hearing rule: you hear exactly the people who can hear you, both
//! being the huddle's verified participants who may be in it right now.

use std::collections::BTreeMap;

use anyhow::Result;
use iroh::PublicKey;

use super::{beacon::media_path_name, book::Participant};
use crate::{
    private::ids::{PrivateKind, private_kind},
    storage::{Ending, ItemKind, Store},
};

/// How many may join a huddle. Two people joining at once can land one over;
/// devices would disagree on any finer ordering.
pub const MAX_PARTICIPANTS: usize = 8;
/// The most audio links a device keeps, whatever the roster says, so a crowd
/// can't overload it.
pub const MAX_LINKS: usize = 16;

/// How a channel's beacons travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Over gossip: any member may see and join a public channel's huddle.
    Workspace,
    /// Directly to the channel's members, so nobody else learns of it.
    Members,
}

/// Who may be in `channel`'s huddle right now, and how its beacons travel;
/// `None` for a channel that doesn't exist here.
pub fn huddle_members(
    store: &Store,
    channel: &str,
    workspace_members: &[PublicKey],
) -> Result<Option<(Vec<PublicKey>, Visibility)>> {
    if store.item_kind(channel)? != Some(ItemKind::Channel) || store.is_deleted(channel)? {
        return Ok(None);
    }
    let still_here = |members: Vec<PublicKey>| -> Vec<PublicKey> {
        members
            .into_iter()
            .filter(|member| workspace_members.contains(member))
            .collect()
    };
    Ok(Some(match private_kind(channel) {
        None => (workspace_members.to_vec(), Visibility::Workspace),
        Some(PrivateKind::Channel) => (
            still_here(store.private_members(channel, Ending::Left)?),
            Visibility::Members,
        ),
        Some(PrivateKind::Dm) => (still_here(store.dm_members(channel)?), Visibility::Members),
    }))
}

/// The devices this device exchanges audio with: every other verified
/// participant who may be in the huddle, at the path their session names.
pub fn hearing_set(
    roster: &[Participant],
    my_device: &PublicKey,
    allowed: &[PublicKey],
) -> BTreeMap<PublicKey, String> {
    roster
        .iter()
        .filter(|participant| participant.endpoint != *my_device && allowed.contains(&participant.member))
        .take(MAX_LINKS)
        .map(|participant| {
            (
                participant.endpoint,
                media_path_name(&participant.endpoint, &participant.session),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::huddle::beacon::{MEDIA_WIRE, Reach};

    fn participant(member: &PublicKey, session: u8) -> Participant {
        Participant {
            member: *member,
            endpoint: *member,
            channel: "general".into(),
            session: [session; 16],
            camera: false,
            screen: false,
            reach: Reach::Direct,
            media_wire: MEDIA_WIRE,
        }
    }

    #[test]
    fn you_hear_the_others_who_may_be_there_and_not_yourself() {
        let [me, bob, carol] = [(); 3].map(|()| SecretKey::generate().public());
        let roster = [participant(&me, 1), participant(&bob, 2), participant(&carol, 3)];
        let heard = hearing_set(&roster, &me, &[me, bob]);
        assert_eq!(heard.keys().copied().collect::<Vec<_>>(), [bob]);
        assert_eq!(heard[&bob], media_path_name(&bob, &[2; 16]));
    }

    #[test]
    fn a_crowd_is_cut_to_the_link_limit() {
        let me = SecretKey::generate().public();
        let crowd: Vec<PublicKey> = (0..MAX_LINKS + 5)
            .map(|_| SecretKey::generate().public())
            .collect();
        let roster: Vec<Participant> = crowd.iter().map(|member| participant(member, 1)).collect();
        assert_eq!(hearing_set(&roster, &me, &crowd).len(), MAX_LINKS);
    }
}
