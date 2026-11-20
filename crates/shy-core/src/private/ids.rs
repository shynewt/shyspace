//! Ids of private channels, DMs and their messages. They contain a `:`,
//! which public ids never do, so ordinary ops can be kept away from them.

use anyhow::Result;
use iroh::PublicKey;

use super::{crypto::Commitment, ops::Invitee};
use crate::ops::{EntityId, new_owned_id};

const CHANNEL_PREFIX: &str = "p:";
const DM_PREFIX: &str = "dm:";
const DM_ID_CONTEXT: &str = "shyspace dm id v1";
const CHANNEL_ID_CONTEXT: &str = "shyspace private channel id v1";
/// Hex characters of the creator's key at the start of a channel id, as in
/// owned ids.
const CREATOR_PREFIX_HEX: usize = 32;
/// Like Slack's group DMs: you plus up to eight others.
pub const MAX_DM_MEMBERS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateKind {
    Channel,
    Dm,
}

/// A private channel's id names its creator and commits to how it starts
/// (its first key, name and members), so only one creation can count.
pub fn private_channel_id(
    creator: &PublicKey,
    commitment: &Commitment,
    name: &str,
    members: &[Invitee],
) -> Result<EntityId> {
    let start = blake3::derive_key(
        CHANNEL_ID_CONTEXT,
        &postcard::to_stdvec(&(commitment, name, members))?,
    );
    Ok(format!(
        "{CHANNEL_PREFIX}{}{}",
        &hex::encode(creator.as_bytes())[..CREATOR_PREFIX_HEX],
        hex::encode(&start[..16])
    ))
}

/// The same people always get the same DM, whoever opens it first.
pub fn dm_id(workspace_id: &str, members: &[PublicKey]) -> EntityId {
    let mut keys: Vec<[u8; 32]> = members.iter().map(|member| *member.as_bytes()).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut hasher = blake3::Hasher::new_derive_key(DM_ID_CONTEXT);
    hasher.update(workspace_id.as_bytes());
    for key in &keys {
        hasher.update(key);
    }
    format!("{DM_PREFIX}{}", hex::encode(&hasher.finalize().as_bytes()[..16]))
}

/// A message's id names its channel, so content can't move between channels.
pub fn new_private_message_id(channel: &str, author: &PublicKey) -> EntityId {
    format!("{channel}/{}", new_owned_id(author))
}

pub fn is_private_id(id: &str) -> bool {
    id.contains(':')
}

pub fn private_kind(channel: &str) -> Option<PrivateKind> {
    if channel.contains('/') {
        None
    } else if channel.starts_with(CHANNEL_PREFIX) {
        Some(PrivateKind::Channel)
    } else if channel.starts_with(DM_PREFIX) {
        Some(PrivateKind::Dm)
    } else {
        None
    }
}

/// The private channel or DM an id belongs to: itself, or a message's channel.
pub fn private_channel_of(id: &str) -> Option<&str> {
    if !is_private_id(id) {
        return None;
    }
    id.split('/').next()
}

pub fn is_creator_of(channel: &str, author: &PublicKey) -> bool {
    channel.strip_prefix(CHANNEL_PREFIX).is_some_and(|rest| {
        rest.len() == 2 * CREATOR_PREFIX_HEX
            && rest.starts_with(&hex::encode(author.as_bytes())[..CREATOR_PREFIX_HEX])
    })
}

/// Whether `id` is a message of `channel`.
pub fn is_message_in(id: &str, channel: &str) -> bool {
    id.strip_prefix(channel)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|owned| !owned.contains('/'))
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::ops::is_owned_by;

    fn channel_of(creator: &PublicKey) -> EntityId {
        private_channel_id(creator, &[7; 32], "plans", &[]).unwrap()
    }

    #[test]
    fn private_ids_never_look_like_public_ones() {
        let ada = SecretKey::generate().public();
        let channel = channel_of(&ada);
        let message = new_private_message_id(&channel, &ada);
        let dm = dm_id("w", &[ada]);
        for id in [&channel, &message, &dm] {
            assert!(is_private_id(id));
        }
        assert!(!is_private_id(&new_owned_id(&ada)));
        assert_eq!(private_kind(&channel), Some(PrivateKind::Channel));
        assert_eq!(private_kind(&dm), Some(PrivateKind::Dm));
        assert_eq!(private_kind(&message), None);
        assert_eq!(private_channel_of(&message), Some(channel.as_str()));
    }

    #[test]
    fn a_channel_id_names_its_creator_and_how_it_starts() {
        let ada = SecretKey::generate().public();
        let bob = SecretKey::generate().public();
        let channel = channel_of(&ada);
        assert!(is_creator_of(&channel, &ada));
        assert!(!is_creator_of(&channel, &bob));
        assert_ne!(channel, private_channel_id(&ada, &[8; 32], "plans", &[]).unwrap());
        assert_ne!(channel, private_channel_id(&ada, &[7; 32], "other", &[]).unwrap());
        let message = new_private_message_id(&channel, &bob);
        assert!(is_owned_by(&message, &bob));
        assert!(!is_owned_by(&message, &ada));
        assert!(is_message_in(&message, &channel));
        assert!(!is_message_in(&message, &channel_of(&bob)));
    }

    #[test]
    fn a_dm_id_depends_only_on_who_is_in_it() {
        let ada = SecretKey::generate().public();
        let bob = SecretKey::generate().public();
        assert_eq!(dm_id("w", &[ada, bob]), dm_id("w", &[bob, ada, bob]));
        assert_ne!(dm_id("w", &[ada, bob]), dm_id("other", &[ada, bob]));
        assert_ne!(dm_id("w", &[ada, bob]), dm_id("w", &[ada]));
    }
}
