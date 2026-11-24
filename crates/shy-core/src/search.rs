//! Search queries: words to find, plus filters. `from:Ada` (quotes for
//! names with spaces, `me` for yourself), `in:#channel`, `before:2026-11-19`
//! and `after:2026-11-01`. Any filter limits results to messages. Also the
//! snippet a result shows.

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Default, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub from: Option<String>,
    pub channel: Option<String>,
    /// Midnight UTC of the given day, in milliseconds.
    pub before_ms: Option<i64>,
    pub after_ms: Option<i64>,
}

impl SearchQuery {
    pub fn has_filters(&self) -> bool {
        self.from.is_some() || self.channel.is_some() || self.before_ms.is_some() || self.after_ms.is_some()
    }
}

/// Words a snippet shows, as many as the search index's own snippets did.
const SNIPPET_WORDS: usize = 12;
/// Words a snippet shows before the first match.
const WORDS_BEFORE_MATCH: usize = 4;

/// A short window of `text` (already one line of what the reader sees)
/// around the first word one of `words` starts, each such word marked
/// \u{1}…\u{2} and a cut end marked "…"; the start when nothing matches.
/// Words are compared ignoring case, not accents, unlike the index.
pub fn snippet(text: &str, words: &str) -> String {
    let wanted: Vec<String> = words
        .split_whitespace()
        .map(|word| word.trim_matches('"').to_lowercase())
        .filter(|word| !word.is_empty())
        .collect();
    let is_match = |word: &str| {
        let word = word.to_lowercase();
        let word = word.trim_start_matches(|character: char| !character.is_alphanumeric());
        wanted.iter().any(|wanted| word.starts_with(wanted.as_str()))
    };
    let all: Vec<&str> = text.split_whitespace().collect();
    let first = all.iter().position(|word| is_match(word)).unwrap_or(0);
    let start = first.saturating_sub(WORDS_BEFORE_MATCH);
    let end = (start + SNIPPET_WORDS).min(all.len());
    let shown: Vec<String> = all[start..end]
        .iter()
        .map(|word| match is_match(word) {
            true => format!("\u{1}{word}\u{2}"),
            false => (*word).to_owned(),
        })
        .collect();
    let mut snippet = shown.join(" ");
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < all.len() {
        snippet.push('…');
    }
    snippet
}

pub fn parse(query: &str) -> Result<SearchQuery> {
    let mut parsed = SearchQuery::default();
    let mut words = Vec::new();
    for token in tokens(query) {
        match token.split_once(':') {
            Some(("from", value)) => parsed.from = Some(value.to_owned()),
            Some(("in", value)) => parsed.channel = Some(value.trim_start_matches('#').to_owned()),
            Some(("before", value)) => parsed.before_ms = Some(day_start_ms(value)?),
            Some(("after", value)) => parsed.after_ms = Some(day_start_ms(value)?),
            _ => words.push(token),
        }
    }
    parsed.text = words.join(" ");
    Ok(parsed)
}

/// Words, keeping `key:"two words"` together (without the quotes).
fn tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut is_quoted = false;
    for c in query.chars() {
        match c {
            '"' => is_quoted = !is_quoted,
            c if c.is_whitespace() && !is_quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn day_start_ms(date: &str) -> Result<i64> {
    let parts: Vec<&str> = date.split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        bail!("dates look like 2026-11-19");
    };
    let (year, month, day): (i64, i64, i64) = (
        year.parse().context("dates look like 2026-11-19")?,
        month.parse().context("dates look like 2026-11-19")?,
        day.parse().context("dates look like 2026-11-19")?,
    );
    ensure!(
        (1..=12).contains(&month) && (1..=31).contains(&day),
        "dates look like 2026-11-19"
    );
    Ok(days_from_civil(year, month, day) * 24 * 60 * 60 * 1000)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_and_words_are_separated() {
        let query = parse(r#"deploy from:"Ada Lovelace" in:#ops after:1970-01-02 notes"#).unwrap();
        assert_eq!(query.text, "deploy notes");
        assert_eq!(query.from.as_deref(), Some("Ada Lovelace"));
        assert_eq!(query.channel.as_deref(), Some("ops"));
        assert_eq!(query.after_ms, Some(24 * 60 * 60 * 1000));
        assert!(parse("before:2026-13-01").is_err());
        assert!(parse("before:yesterday").is_err());
    }

    #[test]
    fn days_are_counted_across_leap_years() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 11, 19), 20_776);
    }
}
