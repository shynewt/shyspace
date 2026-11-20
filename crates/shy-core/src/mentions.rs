//! Mentions are markdown links inside a message's text, so they need no op
//! of their own and read naturally anywhere markdown is shown:
//! `[@Ada](shy://member/<id>)`, `[@channel](shy://mention/channel)` and
//! `[@here](shy://mention/here)`.

const MEMBER_LINK_PREFIX: &str = "shy://member/";
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

/// Longest "@Name" a mention can show: names have up to 80 characters of up to
/// 4 bytes each.
const MAX_MENTION_NAME_BYTES: usize = 4 * 81;
/// Longest "(shy://member/<id>)".
const MAX_MENTION_TARGET_BYTES: usize = 128;

/// `text` with every mention link shown as its `@name`, for previews.
pub fn with_plain_mentions(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[@") {
        let after = &rest[start + 1..];
        match mention_end(after) {
            Some((name_end, close)) => {
                output.push_str(&rest[..start]);
                output.push_str(&after[..name_end]);
                rest = &after[close + 1..];
            }
            None => {
                output.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }
    output.push_str(rest);
    output
}

/// Where the name ends and the link's `)` is, for text after a `[` that
/// starts a mention. Looks only a short way ahead (names and ids are short),
/// so text full of `[@` still takes one pass.
fn mention_end(after: &str) -> Option<(usize, usize)> {
    let name_end = bounded_find(after, ']', MAX_MENTION_NAME_BYTES)?;
    let target = &after[name_end + 1..];
    if !(target.starts_with("(shy://member/") || target.starts_with("(shy://mention/")) {
        return None;
    }
    let close = bounded_find(target, ')', MAX_MENTION_TARGET_BYTES)?;
    Some((name_end, name_end + 1 + close))
}

fn bounded_find(text: &str, wanted: char, max_bytes: usize) -> Option<usize> {
    text.char_indices()
        .take_while(|(index, _)| *index < max_bytes)
        .find(|(_, character)| *character == wanted)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_full_of_mention_starts_is_handled_in_one_pass() {
        let adversarial = format!(
            "{}[@Ada](shy://member/{})",
            "[@x] ".repeat(200_000),
            "a".repeat(64)
        );
        let started = std::time::Instant::now();
        let plain = with_plain_mentions(&adversarial);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert!(
            plain.ends_with("[@x] @Ada"),
            "the real mention at the end still becomes plain"
        );
    }

    #[test]
    fn previews_show_mentions_as_names() {
        let text = format!(
            "hi {} and {CHANNEL_LINK_MARKDOWN} [x](https://a.b) [@odd",
            member_link("Ada", "ab12")
        );
        assert_eq!(
            with_plain_mentions(&text),
            "hi @Ada and @channel [x](https://a.b) [@odd"
        );
    }
}
