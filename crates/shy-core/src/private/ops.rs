//! What travels inside a sealed payload, and inside a key wrap.

use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use super::crypto::{ChannelKey, Commitment};
use crate::ops::{EntityId, Op};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invitee {
    pub member: PublicKey,
    pub after_seq: u64,
}

/// Owner-only ops carry `owner_index`, the owner's place in the channel's
/// line of owners (0 is its creator).
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
        owner_index: u32,
    },
    /// Ends a member's time in the channel after their `last_seq`, and starts
    /// the key generation they don't get.
    Remove {
        member: PublicKey,
        last_seq: u64,
        generation: u64,
        commitment: Commitment,
        owner_index: u32,
    },
    Rotate {
        generation: u64,
        commitment: Commitment,
        owner_index: u32,
    },
    HandOver {
        to: PublicKey,
        owner_index: u32,
    },
    Leave,
    /// A message, reaction, edit, delete or rename inside the channel.
    Content(Op),
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
