//! Read models handed to the UI and to agents. Member ids are hex public keys.

use anyhow::{Context, Result};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::ops::EntityId;

pub fn member_id(member: &PublicKey) -> String {
    hex::encode(member.as_bytes())
}

pub fn parse_member_id(id: &str) -> Result<PublicKey> {
    let bytes: [u8; 32] = hex::decode(id)
        .context("member id is not hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("member id must be 32 bytes"))?;
    Ok(PublicKey::from_bytes(&bytes)?)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Channel {
    pub id: EntityId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub mine: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Message {
    pub id: EntityId,
    pub channel: EntityId,
    pub author: String,
    pub author_name: String,
    pub author_is_bot: bool,
    pub text: String,
    pub reply_to: Option<EntityId>,
    pub created_ms: i64,
    /// Position in the channel's order; pass it back as a cursor to page.
    pub lamport: u64,
    pub reactions: Vec<Reaction>,
}

/// Where to continue when loading older messages: the oldest one loaded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageCursor {
    pub lamport: u64,
    /// The message author's member id (hex).
    pub author: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Page {
    pub id: EntityId,
    pub parent: Option<EntityId>,
    pub title: String,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub is_bot: bool,
    pub is_admin: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub kind: String,
    pub id: EntityId,
    pub title: String,
    /// Matches are wrapped in \u{1} … \u{2} markers for highlighting.
    pub snippet: String,
    pub channel: Option<EntityId>,
}

/// What changed, so listeners refresh only the affected view.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Channels,
    Messages { channel: EntityId },
    Pages,
    Doc { page: EntityId, update: Vec<u8> },
    Members,
    Sync,
    Removed,
}
