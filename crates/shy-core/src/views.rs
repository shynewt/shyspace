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

/// How much of a safety code the app shows to tell same-named members apart:
/// its first two groups ("1234 5678"). No two members' codes may start alike.
pub fn shown_safety_code(code: &str) -> String {
    code.chars().take(9).collect()
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
    pub kind: ChannelKind,
    /// "" for a DM; show its members instead.
    pub name: String,
    /// A DM's members other than you (member ids).
    pub dm_members: Vec<String>,
    /// False once you left or were removed from a private channel; what you
    /// read before stays.
    pub is_member: bool,
    /// A member of a private channel whose owner started a newer key this
    /// device never got: they can't read what's written with it.
    pub is_missing_key: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Public,
    /// Only its members can read it.
    Private,
    Dm,
    /// Notes to yourself: a DM whose only member is you.
    Notes,
}

/// Who is in a private channel, and who owns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateMembers {
    /// Member ids, earliest added first.
    pub members: Vec<String>,
    /// Members who haven't shown yet that they can read it (#110).
    pub invited: Vec<String>,
    pub owner: Option<String>,
    pub is_owner: bool,
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
    /// The level that lets fewer alerts through.
    pub fn stricter(self, other: NotifyLevel) -> NotifyLevel {
        let rank = |level: NotifyLevel| match level {
            NotifyLevel::All => 0,
            NotifyLevel::Mentions => 1,
            NotifyLevel::Nothing => 2,
        };
        if rank(other) > rank(self) { other } else { self }
    }

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
    /// Whether it's in your Later list; personal, never synced.
    pub saved: Option<SavedMark>,
    /// For a thread's first message: how many replies it has, and when the
    /// latest came.
    pub reply_count: u32,
    pub last_reply_ms: Option<i64>,
    /// The latest replies' distinct authors (member ids, hex), newest first,
    /// at most three.
    pub recent_reply_authors: Vec<String>,
    /// The latest reply's author (member id, hex).
    pub last_reply_author: Option<String>,
    /// The latest reply's text on one line, mentions shown as @name, cut to
    /// at most 100 characters (ending in "…" when cut). Still markdown.
    pub last_reply_preview: Option<String>,
    /// Whether this member follows the thread: new replies alert them.
    pub is_following: bool,
    /// Replies by others since this member last read the thread (counted
    /// only when following).
    pub unread_replies: u32,
    pub reactions: Vec<Reaction>,
    /// Files shared with it, in the order they were sent.
    pub files: Vec<FileInfo>,
}

/// A shared file as this member sees it. Its bytes come through
/// `Workspace::file_bytes` or `Workspace::save_file`, or for a page's file
/// through `page_file_bytes` or `save_page_file`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileInfo {
    pub id: EntityId,
    /// The channel, DM or page it was shared in.
    pub place: EntityId,
    /// None for a page's file.
    pub message: Option<EntityId>,
    /// The author's member id (hex).
    pub author: String,
    pub name: String,
    /// As the sender's system named it; only a hint.
    pub media_type: String,
    pub size: u64,
    pub created_ms: i64,
    pub can_delete: bool,
    /// Whether this member may rename it (its sharer).
    pub can_rename: bool,
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
    /// Whether this member saved it for later.
    pub is_saved: bool,
}

/// One editing session's Yjs client id, and the nonce that proves it is
/// this member's.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PageEditorSession {
    pub client_id: u64,
    /// Hex; sent back with each update of the session.
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub is_bot: bool,
    pub is_admin: bool,
    pub active: bool,
    /// Their status, unless none is set or it expired.
    pub status: Option<MemberStatus>,
    /// From live heartbeats; never stored or synced through the bucket.
    pub presence: PresenceState,
    /// What they share about themselves; every field may be empty.
    pub details: crate::profile::ProfileDetails,
    /// Their picture's id, if they shared one; it changes with the picture,
    /// so it can key a cache (`Workspace::profile_picture`).
    pub picture: Option<String>,
    /// The code compared when they joined; it tells apart members who share a name.
    pub safety_code: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SavedMark {
    pub remind_ms: Option<i64>,
    pub is_done: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MemberStatus {
    pub emoji: String,
    pub text: String,
    pub until_ms: Option<i64>,
    /// Which alerts they want while it's set.
    pub alerts: NotifyLevel,
}

/// A status to set for yourself.
pub struct StatusUpdate<'a> {
    pub emoji: &'a str,
    pub text: &'a str,
    pub until_ms: Option<i64>,
    /// Which alerts they want while it's set.
    pub alerts: NotifyLevel,
}

/// Whether a member's device is reachable right now. "Unreachable" rather than
/// "offline": a network that blocks peers looks the same.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceState {
    Online,
    Away,
    #[default]
    Unreachable,
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
    /// Who wrote a message; a page has no single author.
    pub author: Option<String>,
    /// When a message was sent, or a page last changed.
    pub time_ms: i64,
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
    /// Every object sits under this prefix, so bucket rules name it.
    pub workspace_id: String,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    /// Size in bytes of the admin's last recovery snapshot: `None` for
    /// members and before the admin made a kit. Past the limit, the kit
    /// can't restore the workspace.
    pub recovery_snapshot_bytes: Option<u64>,
    /// The largest recovery snapshot restore reads, in bytes.
    pub recovery_snapshot_limit: u64,
}

/// A workspace on this device, as the workspace rail shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceItem {
    pub workspace_id: String,
    pub name: String,
    /// The id of the workspace's icon, which changes with it (load the
    /// bytes with `workspace_icon`); `None` shows a letter tile.
    pub icon: Option<String>,
    /// Checking the bucket at full pace (one of the most recently used);
    /// the others check every few minutes, though gossip still delivers live.
    pub is_live: bool,
    /// Muted on this device: no notifications from it.
    pub is_muted: bool,
}

/// The theme and accent the admin chose for everyone; `None` is the app's
/// default. Names are checked, not interpreted: an app shows a name it
/// doesn't know as its default.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct WorkspaceLook {
    pub theme: Option<String>,
    pub accent: Option<String>,
}

/// What a workspace's rail icon shows: the same counts as its sidebar.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceBadge {
    pub workspace_id: String,
    /// Something unread outside muted channels, or any mention.
    pub unread: bool,
    /// Mentions of you, plus unread messages in DMs that aren't muted.
    pub mentions: u32,
    /// What needs you, counted as the window badge counts: mentions and
    /// followed replies outside DMs, unread messages in unmuted DMs, and
    /// reminders that came due.
    pub attention: u32,
}

/// This device's call, with the workspace it's in and the place's name
/// ("#design", or a DM's people), for showing it while another is selected.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceHuddle {
    pub workspace_id: String,
    pub channel_name: String,
    #[serde(flatten)]
    pub huddle: MyHuddle,
}

/// A call ringing in one of the workspaces.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceRing {
    pub workspace_id: String,
    #[serde(flatten)]
    pub ring: IncomingHuddle,
}

/// A reminder that came due, in one of the workspaces.
#[derive(Debug, Clone, Serialize)]
pub struct DueReminder {
    pub workspace_id: String,
    pub item: LaterItem,
}

/// A change of bucket key under way, after a member was removed (they keep
/// the old key until it is deleted at the provider). Shown to the admin only.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StorageRotation {
    pub provider: StorageProvider,
    /// The key in use is one no removed member had.
    pub has_new_key: bool,
    /// Current members other than the admin, and whether each now uses the
    /// new key (as their head in the bucket says: a hint only).
    pub members: Vec<MemberMove>,
    pub old_key: OldKeyState,
}

/// The storage provider, told from the bucket's endpoint, so the app can
/// show where its keys are made and deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageProvider {
    Scaleway,
    R2,
    B2,
    S3,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberMove {
    pub member: String,
    pub has_moved: bool,
}

/// What the last check of the old key found, while it isn't proved dead.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OldKeyState {
    #[default]
    NotChecked,
    StillWorks,
    /// The check couldn't tell (the bucket was unreachable); tried again.
    Unknown,
}

/// What checking the old key found. `Dead`: every replaced key is refused
/// (the rotation ends unless the key in use is one a removed member had).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OldKeyCheck {
    Dead,
    StillWorks,
    Unknown,
}

/// Whether the bucket keeps deleted or overwritten data, so a member with
/// its keys can't destroy the workspace for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageProtection {
    /// Deletes are refused: safe from accidents and from anyone with the keys.
    Locked,
    /// Old versions are kept: accidents can be undone, but someone with the
    /// keys could still delete them.
    Versioned,
    Unprotected,
    /// Never checked, or the last check failed.
    Unknown,
}

/// The last protection check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectionCheck {
    pub state: StorageProtection,
    pub checked_ms: Option<i64>,
}

/// Another member's cursor on a page; no selection while they aren't in its text.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PageCursor {
    pub member: String,
    pub selection: Option<crate::pages::cursors::CursorSelection>,
    pub idle_ms: i64,
}

/// Who else is on a page (member ids), and their cursors.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct PagePresence {
    pub viewers: Vec<String>,
    pub cursors: Vec<PageCursor>,
}

/// What changed, so listeners refresh only the affected view.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// What's saved for later changed on this device.
    Later,
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
    /// The page's content was rebuilt from scratch; anything holding a copy
    /// of it reloads it.
    PageReset {
        page: EntityId,
    },
    /// A file shared into the page arrived, was renamed or was deleted.
    PageFiles {
        page: EntityId,
    },
    /// Who else is on the page, or where their cursors are, changed.
    PagePresence {
        page: EntityId,
    },
    Members,
    Invites,
    /// The bucket settings changed; the workspace reconnects.
    Storage,
    /// A new result from checking whether the bucket keeps deleted data.
    StorageProtection,
    /// The bucket key rotation (`storage_rotation`) changed.
    KeyRotation,
    /// The workspaces on this device changed: one was added, left, moved,
    /// selected, started or stopped running live, or got a new icon.
    Workspaces,
    /// The workspace's default look changed.
    WorkspaceLook,
    /// How far this device's upload of files into `channel` (or a page) has
    /// got, in bytes of the files themselves.
    Upload {
        channel: EntityId,
        sent: u64,
        total: u64,
    },
    Sync,
    Removed,
    /// Who is in `channel`'s huddle changed.
    Huddle {
        channel: EntityId,
    },
    /// Something about this device's own huddle changed.
    MyHuddle,
    /// Someone started a huddle in a DM you're in: worth one notification.
    /// Raised only for the first to join, and only if the DM may alert you.
    HuddleStarted {
        channel: EntityId,
        /// Member id (hex).
        member: String,
        member_name: String,
    },
    /// An incoming huddle started ringing on this device. Exactly one
    /// `HuddleRingStopped` follows.
    HuddleRinging {
        channel: EntityId,
        /// Member id (hex) of the caller.
        member: String,
        member_name: String,
    },
    /// The ring for an incoming huddle stopped: answered, declined, the
    /// caller left, or nobody answered in time.
    HuddleRingStopped {
        channel: EntityId,
    },
    /// The microphones and speakers offered, or the choice among them, changed.
    HuddleDevices,
    /// Whether huddles play join and leave sounds on this device changed.
    HuddleSounds,
    /// A participant in this device's huddle started or stopped speaking.
    HuddleSpeaking {
        /// Member id (hex); this device's own member for its own voice.
        member: String,
        speaking: bool,
    },
}

/// Who is in a channel's huddle, for everyone who can see the channel.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Huddle {
    pub channel: EntityId,
    pub participants: Vec<HuddleParticipant>,
    pub is_full: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HuddleParticipant {
    /// Member id (hex).
    pub member: String,
    pub camera: bool,
    pub screen: bool,
    pub is_me: bool,
    /// Their app can't exchange audio with this one; one of you must update.
    pub needs_update: bool,
}

/// The microphones and speakers to pick from, and which this device uses.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Devices {
    pub inputs: Vec<crate::huddle::media::AudioDevice>,
    pub outputs: Vec<crate::huddle::media::AudioDevice>,
    /// `None` (the system's default) also when the chosen one went away.
    pub chosen: crate::huddle::media::DeviceChoice,
}

/// The incoming huddle ringing on this device.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IncomingHuddle {
    pub channel: EntityId,
    /// Member id (hex) of the caller.
    pub member: String,
    pub member_name: String,
}

/// This device's own huddle.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MyHuddle {
    pub channel: EntityId,
    pub phase: HuddlePhase,
    pub muted: bool,
    pub mic: crate::huddle::media::MicState,
    pub output: crate::huddle::media::OutputState,
    pub peers: Vec<HuddlePeer>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HuddlePhase {
    Joining,
    In,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HuddlePeer {
    /// Member id (hex).
    pub member: String,
    pub link: crate::huddle::media::Link,
    pub speaking: bool,
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

/// Something saved for later: a message or a page, never both.
#[derive(Debug, Clone, Serialize)]
pub struct LaterItem {
    pub id: EntityId,
    pub saved_ms: i64,
    pub remind_ms: Option<i64>,
    pub is_done: bool,
    pub message: Option<LinkedMessage>,
    pub page: Option<Page>,
}

/// How a sidebar section orders its channels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionSort {
    /// A to Z.
    #[default]
    Name,
    /// Newest message first.
    Recent,
}

impl SectionSort {
    pub fn as_str(self) -> &'static str {
        match self {
            SectionSort::Name => "name",
            SectionSort::Recent => "recent",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "name" => Some(SectionSort::Name),
            "recent" => Some(SectionSort::Recent),
            _ => None,
        }
    }
}

/// A personal group of channels in the sidebar.
#[derive(Debug, Clone, Serialize)]
pub struct SidebarSection {
    pub id: String,
    pub name: String,
    pub sort: SectionSort,
    pub is_collapsed: bool,
    /// The built-in "Channels" section: can't be renamed or deleted.
    pub is_builtin: bool,
    /// Channel ids, in the section's order.
    pub channels: Vec<EntityId>,
}
