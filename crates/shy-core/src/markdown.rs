//! The words a reader sees in markdown, for the search index: link and
//! mention names without their addresses, no markdown marks, no HTML (chat
//! hides it), and no bare app links (`shy://…`), which show as chips.

use pulldown_cmark::{Event, Options, Parser, TagEnd};

const APP_LINK: &str = "shy://";

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
    for event in Parser::new_ext(markdown, options()) {
        match event {
            Event::Text(words) | Event::Code(words) => text.push_str(&words),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::End(TagEnd::TableCell) => text.push(' '),
            Event::End(end) if !is_inline(end) => text.push('\n'),
            _ => {}
        }
    }
    without_app_links(&text)
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
