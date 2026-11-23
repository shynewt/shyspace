//! What travels inside a sealed payload, and inside a key wrap.

use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::crypto::{ChannelKey, Commitment};
use crate::ops::{EntityId, MAX_STORED_NUMBER, Op};

/// Most wraps one grant may carry, so nobody can make every device try
/// thousands of decryptions.
pub const MAX_WRAPS_PER_GRANT: usize = 512;
/// Most members a private channel starts with.
pub const MAX_INITIAL_MEMBERS: usize = 500;

/// The real op inside a sealed payload. `generation` is the key it was sealed
/// with (0 for a DM).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivateBody {
    pub channel: EntityId,
    pub generation: u64,
    pub op: PrivateOp,
}

/// Someone joining a private channel, and the last op of theirs the owner had
/// seen: only their later ops can count as theirs in the channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Invitee {
    pub member: PublicKey,
    pub after_seq: u64,
}

/// Owner ops form the channel's own numbered log: each names its `position`
/// (the creation is 1) and counts only as exactly the next one, written by
/// whoever owns the channel at that position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PrivateOp {
    /// Starts a private channel; only the creator its id names may.
    Create {
        name: String,
        commitment: Commitment,
        members: Vec<Invitee>,
    },
    Add {
        invitee: Invitee,
        from_generation: u64,
        position: u64,
    },
    /// Ends a member's time in the channel after their `last_seq`, and starts
    /// the key generation they don't get.
    Remove {
        member: PublicKey,
        last_seq: u64,
        generation: u64,
        commitment: Commitment,
        position: u64,
    },
    Rotate {
        generation: u64,
        commitment: Commitment,
        position: u64,
    },
    HandOver {
        to: PublicKey,
        position: u64,
    },
    /// The owner deletes a message (or the whole channel).
    OwnerDelete {
        id: EntityId,
        position: u64,
    },
    Leave,
    /// A message, reaction, edit, delete (of the author's own) or rename
    /// inside the channel.
    Content(Op),
    /// Takes over a channel whose owner was removed from the workspace (see
    /// `private/succession.rs`).
    Succeed {
        position: u64,
    },
    /// A member's device has the key and they opened the channel: written
    /// once per membership, so others can tell who really reads it (#110).
    /// The key it's sealed with is the proof.
    Joined,
}

impl PrivateBody {
    /// Whether every number in it fits the store; one that doesn't makes the
    /// op broken, not a local failure that would hold up its author.
    pub fn has_storable_numbers(&self) -> bool {
        let numbers: Vec<u64> = match &self.op {
            PrivateOp::Create { members, .. } => members.iter().map(|invitee| invitee.after_seq).collect(),
            PrivateOp::Add {
                invitee,
                from_generation,
                position,
            } => vec![invitee.after_seq, *from_generation, *position],
            PrivateOp::Remove {
                last_seq,
                generation,
                position,
                ..
            } => vec![*last_seq, *generation, *position],
            PrivateOp::Rotate {
                generation, position, ..
            } => vec![*generation, *position],
            PrivateOp::HandOver { position, .. }
            | PrivateOp::OwnerDelete { position, .. }
            | PrivateOp::Succeed { position } => vec![*position],
            PrivateOp::Leave | PrivateOp::Content(_) | PrivateOp::Joined => Vec::new(),
        };
        std::iter::once(self.generation)
            .chain(numbers)
            .all(|number| number <= MAX_STORED_NUMBER)
    }
}

impl PrivateOp {
    /// Where an owner op sits in the channel's owner log.
    pub fn position(&self) -> Option<u64> {
        match self {
            Self::Create { .. } => Some(1),
            Self::Add { position, .. }
            | Self::Remove { position, .. }
            | Self::Rotate { position, .. }
            | Self::HandOver { position, .. }
            | Self::OwnerDelete { position, .. }
            | Self::Succeed { position } => Some(*position),
            Self::Leave | Self::Content(_) | Self::Joined => None,
        }
    }
}

/// One channel key for one recipient. A DM's grant names its members, which
/// its id is a hash of.
#[derive(Serialize, Deserialize)]
pub struct GrantBody {
    pub channel: EntityId,
    pub generation: u64,
    pub key: ChannelKey,
    pub dm_members: Vec<PublicKey>,
}
