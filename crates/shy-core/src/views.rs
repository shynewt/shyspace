//! Read models handed to the UI and to agents. Member ids are hex public keys.

use anyhow::{Context, Result};
use iroh::PublicKey;
use serde::{Deserialize, Serialize};

use crate::ops::EntityId;

pub fn member_id(member: &PublicKey) -> String {
    hex::encode(member.as_bytes())
}

/// A short code people compare out of band (by phone, in person) to be sure
/// a join request comes from who they think: 20 digits from the identity key.
pub fn safety_code(member: &PublicKey) -> String {
    let digest = blake3::derive_key("shyspace safety code v1", member.as_bytes());
    let number = u128::from_be_bytes(digest[..16].try_into().expect("a 32-byte digest has 16 bytes"));
    let digits = format!("{:020}", number % 10u128.pow(20));
    digits
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_member_id(id: &str) -> Result<PublicKey> {
    let bytes: [u8; 32] = hex::decode(id)
        .context("member id is not hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("member id must be 32 bytes"))?;
    Ok(PublicKey::from_bytes(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_codes_are_stable_twenty_digits() {
        let key = iroh::SecretKey::from_bytes(&[7; 32]).public();
        let code = safety_code(&key);
        assert_eq!(code, safety_code(&key));
        assert_eq!(code.len(), 24);
        assert!(
            code.split(' ')
                .all(|group| group.len() == 4 && group.chars().all(|c| c.is_ascii_digit()))
        );
        assert_ne!(code, safety_code(&iroh::SecretKey::from_bytes(&[8; 32]).public()));
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Channel {
    pub id: EntityId,
    pub name: String,
    /// Whether this member may delete it (its owner or the admin).
    pub can_delete: bool,
    /// Messages by others since this member's read position.
    pub unread: u32,
    /// How many of those mention this member.
    pub unread_mentions: u32,
    /// The last message this member has read, if any.
    pub last_read: Option<MessageCursor>,
    /// Which new messages here alert this member.
    pub notify: NotifyLevel,
}

/// A personal, per-channel choice of which new messages alert you.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    #[default]
    All,
    Mentions,
    Nothing,
}

impl NotifyLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            NotifyLevel::All => "all",
            NotifyLevel::Mentions => "mentions",
            NotifyLevel::Nothing => "nothing",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "all" => Some(NotifyLevel::All),
            "mentions" => Some(NotifyLevel::Mentions),
            "nothing" => Some(NotifyLevel::Nothing),
            _ => None,
        }
    }
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
    pub is_edited: bool,
    /// Whether this member may edit it (its author).
    pub can_edit: bool,
    /// Whether this member may delete it (its author or the admin).
    pub can_delete: bool,
    /// Whether it mentions this member (directly, or @channel / @here).
    pub mentions_me: bool,
    /// For a thread's first message: how many replies it has, and when the
    /// latest came.
    pub reply_count: u32,
    pub last_reply_ms: Option<i64>,
    /// Whether this member follows the thread: new replies alert them.
    pub is_following: bool,
    /// Replies by others since this member last read the thread (counted
    /// only when following).
    pub unread_replies: u32,
    pub reactions: Vec<Reaction>,
}

/// Where to continue when loading older messages: the oldest one loaded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageCursor {
    pub lamport: u64,
    /// The message author's member id (hex).
    pub author: String,
}

impl MessageCursor {
    /// Whether this position comes before `message` in channel order.
    pub fn precedes(&self, message: &Message) -> bool {
        (self.lamport, self.author.as_str()) < (message.lamport, message.author.as_str())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Page {
    pub id: EntityId,
    pub parent: Option<EntityId>,
    pub title: String,
    pub updated_ms: i64,
    /// Whether this member may delete it (its owner or the admin).
    pub can_delete: bool,
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
    /// For a reply, the first message of its thread.
    pub thread_root: Option<EntityId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InviteState {
    /// Waiting for someone to use it.
    Open,
    /// Someone asked to join; the admin decides.
    Requested,
    Accepted,
    Rejected,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Invite {
    pub token: String,
    /// Who the admin made it for, as a reminder ("Claude agent").
    pub label: String,
    pub created_ms: i64,
    pub expires_ms: i64,
    pub state: InviteState,
    pub requester_id: Option<String>,
    pub requester_name: Option<String>,
    pub requester_is_bot: bool,
    /// Compare with the code the requester sees before approving.
    pub requester_safety_code: Option<String>,
}

/// Bucket settings as shown to people: never the secret key.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StorageSettings {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
}

/// What changed, so listeners refresh only the affected view.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// Drafts changed on this device.
    Drafts,
    /// A new message from someone else that this member's level for its
    /// channel says should alert them. Only recent ones: catching up on old
    /// messages never alerts.
    Alert {
        channel: EntityId,
        channel_name: String,
        message: EntityId,
        /// The author's member id (hex) and name ("" until their profile arrives).
        author: String,
        author_name: String,
        /// One line of plain text, mentions shown as @name.
        preview: String,
    },
    Channels,
    Messages {
        channel: EntityId,
    },
    Pages,
    Doc {
        page: EntityId,
        update: Vec<u8>,
    },
    Members,
    Invites,
    /// The bucket settings changed; the workspace reconnects.
    Storage,
    Sync,
    Removed,
}

/// What needs your attention, across channels and threads.
#[derive(Debug, Clone, Serialize)]
pub struct Inbox {
    /// Unread mentions of you and new replies in threads you follow, newest first.
    pub attention: Vec<InboxItem>,
    /// Channels with unread messages.
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InboxItem {
    pub message: Message,
    pub channel_name: String,
    /// The first message of the thread it's in, if it's a reply.
    pub thread_root: Option<EntityId>,
    pub reason: InboxReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxReason {
    Mention,
    ThreadReply,
}

/// A message found from a `shy://message/<id>` link.
#[derive(Debug, Clone, Serialize)]
pub struct LinkedMessage {
    pub message: Message,
    /// The first message of its thread, if it's a reply.
    pub thread_root: Option<EntityId>,
}

/// Unsent text, kept on this device only.
#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    /// A channel, or the first message of the thread being replied in.
    pub target: EntityId,
    /// The channel it's in.
    pub channel: EntityId,
    pub text: String,
    pub updated_ms: i64,
}
