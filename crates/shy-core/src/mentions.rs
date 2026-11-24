//! Mentions are markdown links inside a message's text, so they need no op
//! of their own and read naturally anywhere markdown is shown:
//! `[@Ada](shy://member/<id>)`, `[@channel](shy://mention/channel)` and
//! `[@here](shy://mention/here)`, with links written now also naming their
//! workspace (see `links`).

use crate::links::{APP_LINK, AppLink, parse};

const MEMBER_LINK_PREFIX: &str = "shy://member/";
/// What a link to a member holds, in either form: for finding candidates
/// before parsing them.
pub(crate) const MEMBER_LINK_PART: &str = "member/";
/// What a group mention (@channel, @here) holds, in either form.
pub(crate) const GROUP_MENTION_PART: &str = "mention/";
pub const CHANNEL_LINK_MARKDOWN: &str = "[@channel](shy://mention/channel)";
pub const HERE_LINK_MARKDOWN: &str = "[@here](shy://mention/here)";

/// A link that mentions `member` (hex id), shown as `@name`.
pub fn member_link(name: &str, member: &str) -> String {
    let name: String = name.chars().filter(|c| !matches!(c, '[' | ']')).collect();
    format!("[@{name}]({MEMBER_LINK_PREFIX}{member})")
}

/// Whether `text` mentions `member` of `workspace_id`: by a link to them,
/// or by @channel or @here, in links of this workspace (or older ones, which
/// name none). Until presence exists (#42), @here reaches everyone like
/// @channel.
pub fn mentions(text: &str, workspace_id: &str, member: &str) -> bool {
    app_links_in(text)
        .filter(|link| link.is_in(workspace_id))
        .any(|link| match link.kind {
            "member" => leading_hex(link.rest) == member,
            "mention" => ["channel", "here"]
                .iter()
                .any(|group| link.rest.strip_prefix(group).is_some_and(ends_word)),
            _ => false,
        })
}

fn app_links_in(text: &str) -> impl Iterator<Item = AppLink<'_>> {
    text.match_indices(APP_LINK)
        .filter_map(|(start, _)| parse(&text[start..]))
}

fn leading_hex(text: &str) -> &str {
    let end = text.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(text.len());
    &text[..end]
}

fn ends_word(rest: &str) -> bool {
    !rest.starts_with(|c: char| c.is_ascii_alphanumeric())
}
