//! Mentions are markdown links inside a message's text, so they need no op
//! of their own and read naturally anywhere markdown is shown:
//! `[@Ada](shy://member/<id>)`, `[@channel](shy://mention/channel)` and
//! `[@here](shy://mention/here)`.

pub(crate) const MEMBER_LINK_PREFIX: &str = "shy://member/";
/// What every mention of a group (@channel, @here) starts with.
pub(crate) const GROUP_MENTION_PREFIX: &str = "shy://mention/";
const CHANNEL_LINK: &str = "shy://mention/channel";
const HERE_LINK: &str = "shy://mention/here";
pub const CHANNEL_LINK_MARKDOWN: &str = "[@channel](shy://mention/channel)";
pub const HERE_LINK_MARKDOWN: &str = "[@here](shy://mention/here)";

/// A link that mentions `member` (hex id), shown as `@name`.
pub fn member_link(name: &str, member: &str) -> String {
    let name: String = name.chars().filter(|c| !matches!(c, '[' | ']')).collect();
    format!("[@{name}]({MEMBER_LINK_PREFIX}{member})")
}

/// Whether `text` mentions `member`: by a link to them, or by @channel or
/// @here. Until presence exists (#42), @here reaches everyone like @channel.
pub fn mentions(text: &str, member: &str) -> bool {
    text.contains(CHANNEL_LINK) || text.contains(HERE_LINK) || mentioned_members(text).any(|id| id == member)
}

fn mentioned_members(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices(MEMBER_LINK_PREFIX).map(|(start, prefix)| {
        let rest = &text[start + prefix.len()..];
        let end = rest.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(rest.len());
        &rest[..end]
    })
}
