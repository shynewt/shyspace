use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use iroh::PublicKey;

use crate::transport::bucket::BucketConfig;
use serde::{Deserialize, Serialize};

pub type EntityId = String;

pub fn now_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is set before 1970")?;
    Ok(i64::try_from(elapsed.as_millis())?)
}

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
    /// New bucket settings (for example rotated keys). Admin only; members
    /// ignore it from anyone else. Removed members can't read it.
    StorageSettings {
        bucket: BucketConfig,
    },
    /// New text for a message. Only its author may edit it.
    MessageEdit {
        id: EntityId,
        text: String,
    },
    /// Anyone may rename a channel; the latest rename wins.
    ChannelRename {
        id: EntityId,
        name: String,
    },
    /// Deletes whatever `id` names: a message, a channel (with its messages)
    /// or a page. Deletes are permanent and win over everything else about
    /// their target, whatever order ops arrive in. The owner or the admin
    /// may delete.
    Delete {
        id: EntityId,
    },
    /// The author's own status: emoji and text, until a time, and whether
    /// they're busy (no alerts). Empty emoji and text clear it.
    Status {
        emoji: String,
        text: String,
        until_ms: Option<i64>,
        is_busy: bool,
    },
}

/// An op that is validly signed but breaks a rule (someone else's id, an
/// admin-only change, a damaged page update). Every replica reaches the same
/// verdict, so it is skipped rather than retried. Other errors (a full disk,
/// say) are not rule violations and must not cause a skip.
#[derive(Debug)]
pub struct RuleViolation(pub String);

impl std::fmt::Display for RuleViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RuleViolation {}

pub(crate) fn rule_violation(reason: impl Into<String>) -> anyhow::Error {
    RuleViolation(reason.into()).into()
}

impl Op {
    /// Whether `author` may make this change in a workspace run by `admin`.
    pub fn is_permitted(&self, author: &PublicKey, admin: &PublicKey) -> bool {
        match self {
            Op::StorageSettings { .. } => author == admin,
            Op::MessageEdit { id, .. } => is_owned_by(id, author),
            Op::Delete { id } => author == admin || is_owned_by(id, author),
            _ => true,
        }
    }
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

const MAX_CHANNEL_NAME_CHARS: usize = 80;
const MAX_STATUS_TEXT_CHARS: usize = 100;
const MAX_STATUS_EMOJI_CHARS: usize = 16;

/// A status shows next to the author's name everywhere: bounded, on one
/// line, and free of characters that hide or reorder text. Emoji keep their
/// joiners (U+200D) and variation selectors.
pub fn is_valid_status(emoji: &str, text: &str) -> bool {
    let is_sneaky = |c: char| {
        c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}')
            || (c != '\u{200D}' && is_invisible_format_char(c))
    };
    emoji.chars().count() <= MAX_STATUS_EMOJI_CHARS
        && text.chars().count() <= MAX_STATUS_TEXT_CHARS
        && !emoji.chars().chain(text.chars()).any(is_sneaky)
}

/// Turns what someone typed into a channel name: lowercase, words joined by
/// dashes, no leading `#`.
pub fn normalize_channel_name(raw: &str) -> anyhow::Result<String> {
    let name = raw
        .trim()
        .trim_start_matches('#')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");
    anyhow::ensure!(
        is_valid_channel_name(&name),
        "channel names use letters, numbers, dashes, dots and underscores (up to {MAX_CHANNEL_NAME_CHARS})"
    );
    Ok(name)
}

/// Renames from anyone must follow the rule new names follow. Names reach
/// everyone's sidebar: no control or invisible characters, bounded length.
pub fn is_valid_channel_name(name: &str) -> bool {
    let length = name.chars().count();
    (1..=MAX_CHANNEL_NAME_CHARS).contains(&length)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Channels created before the naming rule may hold any text, and every
/// member must still list them the same way: instead of rejecting a
/// created name, drop control and invisible characters and cut it short.
pub fn clean_channel_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !is_invisible_format_char(*c))
        .take(MAX_CHANNEL_NAME_CHARS)
        .collect()
}

/// Zero-width and text-direction characters, which can disguise a name.
fn is_invisible_format_char(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}
