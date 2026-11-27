//! The words a reader sees in markdown: for the search index, link and
//! mention labels without their addresses, no markdown marks, no HTML (chat
//! hides it), and no bare app links (`shy://…`), which show as chips; for
//! previews, the same with every app link named by its real target, as its
//! chip names it.

use std::collections::HashMap;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::links::{APP_LINK, AppLink, parse};
/// Shown before a page link's title and a message link's words in previews,
/// as their chips show an icon.
const PAGE_ICON: &str = "📄";
const MESSAGE_ICON: &str = "💬";

/// The same flavour the desktop renders: GitHub's tables, strikethrough,
/// task lists, footnotes and callouts.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
}

pub(crate) fn visible_text(markdown: &str) -> String {
    let mut text = String::with_capacity(markdown.len());
    let mut images_open = 0_usize;
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Start(Tag::Image { .. }) => images_open += 1,
            Event::End(TagEnd::Image) => images_open = images_open.saturating_sub(1),
            _ if images_open > 0 => {}
            Event::Text(words) | Event::Code(words) => text.push_str(&words),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::End(TagEnd::TableCell) => text.push(' '),
            Event::End(end) if !is_inline(end) => text.push('\n'),
            _ => {}
        }
    }
    without_app_links(&text)
}

/// Who and what app links point to, for naming them in previews.
pub(crate) struct LinkNames {
    /// The workspace these names are of; links into another aren't named.
    pub(crate) workspace_id: String,
    /// Member id (hex) to name.
    pub(crate) members: HashMap<String, String>,
    /// Page id to title.
    pub(crate) pages: HashMap<String, String>,
}

impl LinkNames {
    /// What a chip for `link` says, and the text after the link's id (a
    /// bare link can run into punctuation); `None` for a link no chip names.
    fn name(&self, link: &str) -> Option<(String, String)> {
        let app_link = parse(link)?;
        let link_is_here = app_link.is_in(&self.workspace_id);
        let AppLink { kind, rest, .. } = app_link;
        let id_end = match kind {
            "message" => message_id_end(rest),
            _ => rest
                .find(|character: char| !character.is_ascii_alphanumeric())
                .unwrap_or(rest.len()),
        };
        let (id, after) = rest.split_at(id_end);
        if !link_is_here {
            return Some(("a link to another workspace".to_owned(), after.to_owned()));
        }
        let name = match kind {
            "member" => {
                let name = self.members.get(id).map(|name| name.trim()).unwrap_or_default();
                match name.is_empty() {
                    true => format!("@member {}", id.get(..6).unwrap_or(id)),
                    false => format!("@{name}"),
                }
            }
            "mention" if id == "channel" || id == "here" => format!("@{id}"),
            "page" => {
                let title = self.pages.get(id).map(|title| title.trim()).unwrap_or_default();
                match title.is_empty() {
                    true => format!("{PAGE_ICON} Untitled page"),
                    false => format!("{PAGE_ICON} {title}"),
                }
            }
            "message" => format!("{MESSAGE_ICON} a message"),
            _ => return None,
        };
        Some((name, after.to_owned()))
    }
}

/// What a reader sees of `markdown` on one line, with every app link named
/// by its real target: never its author's label, and found through the
/// parser, so a reference link resolves wherever its definition is.
pub(crate) fn preview_text(markdown: &str, names: &LinkNames) -> String {
    plain_text(markdown, names, Layout::OneLine)
}

/// What a reader sees of `markdown` as a message's body: as [`preview_text`],
/// but keeping its lines, paragraphs, list items (as "• ") and code lines.
pub(crate) fn body_text(markdown: &str, names: &LinkNames) -> String {
    plain_text(markdown, names, Layout::Body)
}

#[derive(Clone, Copy, PartialEq)]
enum Layout {
    OneLine,
    Body,
}

fn plain_text(markdown: &str, names: &LinkNames, layout: Layout) -> String {
    let mut text = String::with_capacity(markdown.len());
    let mut is_in_named_link = false;
    let mut images_open = 0_usize;
    let line_break = match layout {
        Layout::OneLine => ' ',
        Layout::Body => '\n',
    };
    for event in Parser::new_ext(markdown, options()) {
        match event {
            // Chat shows no images, so their alt text isn't seen either.
            Event::Start(Tag::Image { .. }) => images_open += 1,
            Event::End(TagEnd::Image) => images_open = images_open.saturating_sub(1),
            _ if images_open > 0 => {}
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some((name, _)) = names.name(&dest_url) {
                    text.push_str(&name);
                    is_in_named_link = true;
                }
            }
            Event::End(TagEnd::Link) => is_in_named_link = false,
            _ if is_in_named_link => {}
            Event::Text(words) | Event::Code(words) => text.push_str(&with_bare_links_named(&words, names)),
            Event::SoftBreak | Event::HardBreak => text.push(line_break),
            Event::Start(Tag::Item) if layout == Layout::Body => text.push_str("• "),
            Event::End(end) if !is_inline(end) => text.push_str(block_end(end, layout)),
            _ => {}
        }
    }
    match layout {
        Layout::OneLine => {
            // Control characters would fake a snippet's match marks, or reach a
            // system notification.
            let text: String = text.chars().filter(|character| !character.is_control()).collect();
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }
        Layout::Body => tidy_lines(&text),
    }
}

/// What follows a block: a space on one line; in a body, a line break after a
/// list item or table row and a blank line after anything else.
fn block_end(end: TagEnd, layout: Layout) -> &'static str {
    match (layout, end) {
        (Layout::OneLine, _) | (Layout::Body, TagEnd::TableCell) => " ",
        (Layout::Body, TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead) => "\n",
        (Layout::Body, _) => "\n\n",
    }
}

/// A body's lines without control characters or trailing spaces, at most one
/// blank line in a row, and none at the start or end. A line's leading spaces
/// stay, so code keeps its indent.
fn tidy_lines(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line: String = line.chars().filter(|character| !character.is_control()).collect();
        let line = line.trim_end();
        let is_second_blank = line.is_empty() && lines.last().is_none_or(|previous| previous.is_empty());
        if !is_second_blank {
            lines.push(line.to_owned());
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Where a message id ends in `text`: an optional `p:<hex>/` or `dm:<hex>/`
/// (a private channel's or DM's message), then hex, as the desktop reads it.
fn message_id_end(text: &str) -> usize {
    let hex_end = |from: usize| {
        text[from..]
            .find(|character: char| !character.is_ascii_hexdigit())
            .map_or(text.len(), |end| from + end)
    };
    let place = ["p:", "dm:"]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix).map(|_| prefix.len()))
        .and_then(|start| {
            let end = hex_end(start);
            (end > start && text[end..].starts_with('/')).then_some(end + 1)
        });
    hex_end(place.unwrap_or(0))
}

/// `text` with each bare app link replaced by its name, or dropped if no
/// chip names it.
fn with_bare_links_named(text: &str, names: &LinkNames) -> String {
    let mut named = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(APP_LINK) {
        named.push_str(&rest[..start]);
        let link = &rest[start..];
        let end = link.find(char::is_whitespace).unwrap_or(link.len());
        if let Some((name, after)) = names.name(&link[..end]) {
            named.push_str(&name);
            named.push_str(&after);
        }
        rest = &link[end..];
    }
    named.push_str(rest);
    named
}

fn is_inline(end: TagEnd) -> bool {
    matches!(
        end,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
    )
}

fn without_app_links(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(APP_LINK) {
        kept.push_str(&rest[..start]);
        let link = &rest[start..];
        let end = link.find(char::is_whitespace).unwrap_or(link.len());
        rest = &link[end..];
    }
    kept.push_str(rest);
    kept.truncate(kept.trim_end().len());
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> LinkNames {
        LinkNames {
            workspace_id: "0123456789abcdef".to_owned(),
            members: [("ab12".to_owned(), "Eve".to_owned())].into(),
            pages: [("p1".to_owned(), "Plan".to_owned())].into(),
        }
    }

    #[test]
    fn previews_name_app_links_by_their_target_and_keep_other_labels() {
        let text = "hi [@Ada](shy://member/ab12) and [@channel](shy://mention/channel)\n\n\
                    [site](https://a.b), [doc](shy://page/p1) and <shy://page/p1>; [file](shy://file/f1)";
        assert_eq!(
            preview_text(text, &names()),
            "hi @Eve and @channel site, 📄 Plan and 📄 Plan; file"
        );
    }

    #[test]
    fn images_show_nothing_in_previews_or_the_index() {
        let text = "see ![@Boss](shy://member/ab12) ![photo](https://a.b/c.png) now";
        assert_eq!(preview_text(text, &names()), "see now");
        assert_eq!(words(text), ["see", "now"]);
    }

    #[test]
    fn control_characters_never_reach_a_preview() {
        let text = "\u{1}fake\u{2} mark \u{7}bell\u{1b}[31m";
        assert_eq!(preview_text(text, &names()), "fake mark bell[31m");
    }

    #[test]
    fn a_private_message_link_is_named_whole() {
        let text = "see shy://message/p:abc123/def456. and [it](shy://message/dm:ab/cd)";
        assert_eq!(preview_text(text, &names()), "see 💬 a message. and 💬 a message");
    }

    #[test]
    fn links_naming_this_workspace_are_named_like_older_ones() {
        let here = "0123456789abcdef";
        let text = format!(
            "hi [@Ada](shy://{here}/member/ab12), [doc](shy://{here}/page/p1) and shy://{here}/page/p1; \
             see shy://{here}/message/p:abc123/def456. and [it](shy://{here}/message/dm:ab/cd)"
        );
        assert_eq!(
            preview_text(&text, &names()),
            "hi @Eve, 📄 Plan and 📄 Plan; see 💬 a message. and 💬 a message"
        );
    }

    #[test]
    fn links_into_another_workspace_never_show_their_label() {
        let text = "[@Eve](shy://fedcba9876543210/member/ab12) or shy://fedcba9876543210/page/p1.";
        assert_eq!(
            preview_text(text, &names()),
            "a link to another workspace or a link to another workspace."
        );
    }

    #[test]
    fn odd_workspace_parts_read_as_older_links_without_panicking() {
        for text in [
            "[x](shy://0123456789ABCDEF/page/p1)",
            "[x](shy://0123456789abcde/page/p1)",
            "[x](shy://0123456789abcdef0/page/p1)",
            "[x](shy://0123456789abcdef)",
            "[x](shy://0123456789abcdef/)",
            "[x](shy://0123456789abcdef/page)",
            "shy://0123456789abcdef/message/ shy:// shy:///",
        ] {
            let preview = preview_text(text, &names());
            assert!(
                !preview.contains("Plan") || text.contains("abcdef/page/p1"),
                "{text}: {preview}"
            );
            words(text);
        }
    }

    #[test]
    fn new_and_older_bare_links_stay_out_of_the_index() {
        assert_eq!(
            words("see shy://0123456789abcdef/page/p1 and shy://page/p2 now"),
            ["see", "and", "now"]
        );
    }

    #[test]
    fn text_full_of_link_starts_previews_quickly() {
        let adversarial = format!(
            "{}[@Ada](shy://member/ab12)",
            "[@x] [".repeat(crate::ops::MAX_MESSAGE_CHARS / 6)
        );
        let started = std::time::Instant::now();
        let preview = preview_text(&adversarial, &names());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert!(preview.ends_with("@Eve"), "the real link at the end is named");
    }

    fn words(markdown: &str) -> Vec<String> {
        visible_text(markdown)
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn marks_go_and_words_stay() {
        assert_eq!(
            words("# Plan\n\n**Ship** _it_ ~~now~~ `soon`\n\n> quoted"),
            ["Plan", "Ship", "it", "now", "soon", "quoted"]
        );
        assert_eq!(
            words("- [x] done\n- [ ] next\n1. first"),
            ["done", "next", "first"]
        );
        assert_eq!(words("```rust\nlet x = 1;\n```"), ["let", "x", "=", "1;"]);
        assert_eq!(words("> [!NOTE]\n> heads up"), ["heads", "up"]);
    }

    #[test]
    fn links_keep_their_names_but_not_their_addresses() {
        assert_eq!(
            words("[@Ada](shy://member/abc) read [the sheet](https://sheets.example/q3)"),
            ["@Ada", "read", "the", "sheet"]
        );
        assert_eq!(
            words("see shy://page/abc_def, or [](shy://page/abc)"),
            ["see", "or"]
        );
        assert_eq!(
            words("<shy://message/abc> and <https://a.example>"),
            ["and", "https://a.example"]
        );
        assert_eq!(
            words("plain https://a.example/x"),
            ["plain", "https://a.example/x"]
        );
    }

    #[test]
    fn blocks_and_cells_stay_apart() {
        assert_eq!(words("one\ntwo\n# three\nfour"), ["one", "two", "three", "four"]);
        assert_eq!(words("| a | b |\n|---|---|\n| c | d |"), ["a", "b", "c", "d"]);
    }

    #[test]
    fn html_is_hidden_like_in_chat() {
        assert_eq!(words("a <b>bold</b> word"), ["a", "bold", "word"]);
        assert_eq!(words("<div>\nhidden\n</div>\n\nshown"), ["shown"]);
    }
}
