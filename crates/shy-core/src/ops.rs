use iroh::PublicKey;
use serde::{Deserialize, Serialize};

pub type EntityId = String;

const OWNER_PREFIX_BYTES: usize = 16;

pub fn new_random_id() -> String {
    hex::encode(rand::random::<[u8; 8]>())
}

/// Ids of channels, messages and pages start with their creator's key, so
/// nobody else can claim (and overwrite) an id they didn't create.
pub fn new_owned_id(author: &PublicKey) -> EntityId {
    format!("{}{}", owner_prefix(author), new_random_id())
}

pub fn is_owned_by(id: &str, author: &PublicKey) -> bool {
    id.len() == 2 * OWNER_PREFIX_BYTES + 16 && id.starts_with(&owner_prefix(author))
}

fn owner_prefix(author: &PublicKey) -> String {
    hex::encode(&author.as_bytes()[..OWNER_PREFIX_BYTES])
}

/// A single change to the workspace. Every variant merges without needing a
/// global order: names and titles are last-writer-wins by lamport clock,
/// reactions are per-author toggles, and doc updates are Yjs CRDT updates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Op {
    Profile {
        name: String,
        is_bot: bool,
    },
    ChannelCreate {
        id: EntityId,
        name: String,
    },
    Message {
        id: EntityId,
        channel: EntityId,
        text: String,
        reply_to: Option<EntityId>,
    },
    Reaction {
        message: EntityId,
        emoji: String,
        active: bool,
    },
    PageCreate {
        id: EntityId,
        parent: Option<EntityId>,
        title: String,
    },
    PageRename {
        id: EntityId,
        title: String,
    },
    DocUpdate {
        page: EntityId,
        update: Vec<u8>,
    },
}

/// What the author signs and the epoch key encrypts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpBody {
    pub lamport: u64,
    pub timestamp_ms: i64,
    pub op: Op,
}

/// The unit that travels over gossip and sits in the bucket. Only the header
/// is readable without the workspace keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub author: PublicKey,
    pub seq: u64,
    pub epoch: u64,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct VerifiedOp {
    pub author: PublicKey,
    pub seq: u64,
    pub body: OpBody,
}
