//! Read models handed to the UI and to agents. Member ids are hex public keys.

use iroh::PublicKey;
use serde::Serialize;

use crate::ops::EntityId;

pub fn member_id(member: &PublicKey) -> String {
    hex::encode(member.as_bytes())
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
    pub reactions: Vec<Reaction>,
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
