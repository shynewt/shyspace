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

/// Where the last mention of `member` of `workspace_id` in `text` starts,
/// by a link to them (not by @channel or @here): at its `[@label]` if the
/// link has one right before it. Linear in the text, however many links it
/// holds.
pub fn last_member_mention_at(text: &str, workspace_id: &str, member: &str) -> Option<usize> {
    text.rmatch_indices(APP_LINK)
        .find(|(start, _)| {
            parse(&text[*start..]).is_some_and(|link| {
                link.is_in(workspace_id) && link.kind == "member" && leading_hex(link.rest) == member
            })
        })
        .map(|(start, _)| label_start(text, start).unwrap_or(start))
}

/// Labels this long or longer aren't looked for: mentions write a name.
const MAX_LABEL_BYTES: usize = 256;

/// Where the `[label]` of the markdown link whose address starts at `address` begins.
fn label_start(text: &str, address: usize) -> Option<usize> {
    let before = text[..address].strip_suffix("](")?;
    let earliest = (before.len().saturating_sub(MAX_LABEL_BYTES)..=before.len())
        .find(|at| before.is_char_boundary(*at))?;
    let open = earliest + before[earliest..].rfind('[')?;
    (!before[open..].contains(['\n', ']'])).then_some(open)
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

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "0123456789abcdef";
    const ADA: &str = "aa00000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn the_last_member_mention_is_found_at_its_label() {
        let text = format!("Hi [@Ada](shy://{WORKSPACE}/member/{ADA}) and [@Ada](shy://member/{ADA}).");
        assert_eq!(
            last_member_mention_at(&text, WORKSPACE, ADA),
            text.rfind("[@Ada]")
        );
        let bare = format!("[@Ada](shy://member/{ADA}) then shy://member/{ADA}");
        assert_eq!(
            last_member_mention_at(&bare, WORKSPACE, ADA),
            bare.rfind("shy://")
        );
    }

    #[test]
    fn mentions_of_others_elsewhere_or_everyone_are_not_found() {
        for text in [
            format!("[@Ada](shy://member/{ADA}b)"),
            format!("[@Ada](shy://ffffffffffffffff/member/{ADA})"),
            "[@here](shy://mention/here) [@channel](shy://mention/channel)".to_owned(),
        ] {
            assert_eq!(last_member_mention_at(&text, WORKSPACE, ADA), None, "{text}");
        }
    }

    #[test]
    fn a_text_full_of_links_takes_linear_time() {
        let other = "bb".repeat(32);
        let mut text = format!("](shy://member/{other}").repeat(20_000);
        text.push_str(&format!("](shy://member/{ADA}"));
        let started = std::time::Instant::now();
        assert_eq!(
            last_member_mention_at(&text, WORKSPACE, ADA),
            text.rfind("shy://")
        );
        let mine_everywhere = format!("](shy://member/{ADA}").repeat(20_000);
        assert!(last_member_mention_at(&mine_everywhere, WORKSPACE, ADA).is_some());
        // Quadratic work on these would take minutes; linear takes milliseconds.
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
