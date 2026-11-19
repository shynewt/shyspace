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

/// `text` with every mention link shown as its `@name`, for previews.
pub fn with_plain_mentions(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[@") {
        let after = &rest[start + 1..];
        let link = after
            .find("](shy://member/")
            .or_else(|| after.find("](shy://mention/"))
            .filter(|end| !after[..*end].contains(']'))
            .and_then(|end| after[end..].find(')').map(|close| (end, end + close)));
        match link {
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

#[cfg(test)]
mod tests {
    use super::*;

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
