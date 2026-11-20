use std::time::{SystemTime, UNIX_EPOCH};

use crate::views::NotifyLevel;
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

/// A private message's id names its channel first; the owned part is last.
pub fn is_owned_by(id: &str, author: &PublicKey) -> bool {
    let owned = match crate::private::ids::is_private_id(id) {
        true => id.rsplit(['/', ':']).next().unwrap_or(id),
        false => id,
    };
    owned.len() == 2 * OWNER_PREFIX_BYTES + 16 && owned.starts_with(&owner_prefix(author))
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
    /// The author's own status: emoji and text, until a time, and which
    /// alerts they want meanwhile. Empty emoji and text clear it.
    Status {
        emoji: String,
        text: String,
        until_ms: Option<i64>,
        alerts: StatusAlerts,
    },
    /// The author's public key for receiving private channel keys. The latest
    /// one wins; their device keeps every secret it announced.
    SealingKey {
        public: Vec<u8>,
    },
    /// Private channel keys, each wrapped for one recipient.
    KeyGrant {
        wraps: Vec<crate::private::crypto::Wrap>,
    },
    /// An op in a private channel or DM, readable only by its members.
    Sealed {
        payload: crate::private::crypto::SealedPayload,
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
    /// Every id this op creates, changes or points at.
    pub fn named_ids(&self) -> Vec<&str> {
        match self {
            Op::ChannelCreate { id, .. }
            | Op::PageRename { id, .. }
            | Op::MessageEdit { id, .. }
            | Op::ChannelRename { id, .. }
            | Op::Delete { id } => vec![id],
            Op::Message {
                id,
                channel,
                reply_to,
                ..
            } => [Some(id), Some(channel), reply_to.as_ref()]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect(),
            Op::Reaction { message, .. } => vec![message],
            Op::PageCreate { id, parent, .. } => [Some(id), parent.as_ref()]
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect(),
            Op::DocUpdate { page, .. } => vec![page],
            Op::Profile { .. }
            | Op::StorageSettings { .. }
            | Op::Status { .. }
            | Op::SealingKey { .. }
            | Op::KeyGrant { .. }
            | Op::Sealed { .. } => Vec::new(),
        }
    }

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

/// The most a sealed page update may take: room for a very large paste.
pub const MAX_PAGE_UPDATE_BYTES: usize = 4 * 1024 * 1024;
/// The most any other sealed op may take. The longest message, or a key
/// grant to the most people, needs well under half of it.
pub const MAX_OP_BYTES: usize = 256 * 1024;

impl Envelope {
    /// Whether this envelope, holding `op`, is within the size limit for its
    /// kind. Others refuse bigger ones, so every device keeps the same log.
    pub fn fits_size_limit(&self, op: &Op) -> bool {
        let limit = match op {
            Op::DocUpdate { .. } => MAX_PAGE_UPDATE_BYTES,
            _ => MAX_OP_BYTES,
        };
        self.ciphertext.len() <= limit
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedOp {
    pub author: PublicKey,
    pub seq: u64,
    /// The membership epoch it was sealed in.
    pub epoch: u64,
    pub body: OpBody,
}

const MAX_CHANNEL_NAME_CHARS: usize = 80;
/// Longest message or edit, like Slack's. Longer text from others is cut
/// rather than refused, so devices that accepted it before this limit and
/// devices after it don't disagree about whether the message exists.
pub const MAX_MESSAGE_CHARS: usize = 40_000;
pub const MAX_PAGE_TITLE_CHARS: usize = 200;

/// `text` cut to at most `max` characters, the same way on every device.
pub(crate) fn cut_to(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}
const MAX_STATUS_TEXT_CHARS: usize = 100;
pub const MAX_PROFILE_NAME_CHARS: usize = 80;
/// Enough for any emoji sequence, and for a custom emoji's `:name:` later.
pub const MAX_EMOJI_CHARS: usize = 32;
/// The longest id an op may name. Real ones are under about 120 (a private
/// message's), so only forged ops come near.
pub const MAX_ID_CHARS: usize = 200;
/// How much of a page goes into the search index.
pub const MAX_INDEXED_PAGE_CHARS: usize = 200_000;

/// Emoji keep their joiners (U+200D); everything else invisible goes.
fn is_visible_in_emoji(c: char) -> bool {
    (!c.is_control() && !is_invisible_format_char(c)) || c == '\u{200D}'
}

/// An emoji from someone else, as every device shows it: without control or
/// invisible characters, cut short.
pub fn clean_emoji(raw: &str) -> String {
    raw.chars()
        .filter(|c| is_visible_in_emoji(*c))
        .take(MAX_EMOJI_CHARS)
        .collect()
}

/// A display name from someone else, as every device shows it. Besides what
/// `clean_emoji` drops, tag characters go: they only matter inside flag
/// emoji, and in a name they're invisible.
pub fn clean_display_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| is_visible_in_emoji(*c) && !('\u{E0020}'..='\u{E007F}').contains(c))
        .take(MAX_PROFILE_NAME_CHARS)
        .collect()
}

const MAX_STATUS_EMOJI_CHARS: usize = 16;

/// Which alerts a status lets through. The variant order is part of the
/// format: earlier builds stored a busy flag here, and `false`/`true` read as
/// `All`/`Nothing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusAlerts {
    All = 0,
    Nothing = 1,
    Mentions = 2,
}

impl From<NotifyLevel> for StatusAlerts {
    fn from(level: NotifyLevel) -> Self {
        match level {
            NotifyLevel::All => StatusAlerts::All,
            NotifyLevel::Mentions => StatusAlerts::Mentions,
            NotifyLevel::Nothing => StatusAlerts::Nothing,
        }
    }
}

impl TryFrom<u8> for StatusAlerts {
    type Error = anyhow::Error;

    fn try_from(stored: u8) -> anyhow::Result<Self> {
        match stored {
            0 => Ok(StatusAlerts::All),
            1 => Ok(StatusAlerts::Nothing),
            2 => Ok(StatusAlerts::Mentions),
            other => anyhow::bail!("unknown status alert level {other}"),
        }
    }
}

impl From<StatusAlerts> for NotifyLevel {
    fn from(alerts: StatusAlerts) -> Self {
        match alerts {
            StatusAlerts::All => NotifyLevel::All,
            StatusAlerts::Mentions => NotifyLevel::Mentions,
            StatusAlerts::Nothing => NotifyLevel::Nothing,
        }
    }
}

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
            // Blank-looking fillers, joiners and separators.
            | '\u{034F}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{3164}'
            | '\u{FFA0}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0100}'..='\u{E01EF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_saved_as_busy_or_not_still_read_the_same() {
        // Earlier builds stored a busy flag where the alert level now is.
        for (was_busy, alerts) in [(false, StatusAlerts::All), (true, StatusAlerts::Nothing)] {
            let old = postcard::to_stdvec(&("🎧", "Focus", None::<i64>, was_busy)).unwrap();
            let new = postcard::to_stdvec(&("🎧", "Focus", None::<i64>, alerts)).unwrap();
            assert_eq!(old, new);
        }
    }
}
