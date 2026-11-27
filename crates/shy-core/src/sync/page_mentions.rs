//! Mentions of you that others add to pages. Each goes to the inbox, and a
//! recent one alerts like a chat mention. Only text an update inserts
//! counts, so mentions already on a page never do, and @channel and @here
//! mean nothing in a page.
//!
//! Finding a mention reads only the update, never the page: its preview is
//! the inserted text around it, so neither alerts nor the inbox load pages.

use std::collections::{BTreeMap, VecDeque};

use anyhow::Result;
use iroh::PublicKey;
use tracing::warn;

use super::alerts::{ALERT_WINDOW_MS, PREVIEW_CHARS};
use crate::{
    markdown::{LinkNames, preview_text},
    mentions::last_member_mention_at,
    ops::{Op, VerifiedOp, now_ms},
    pages,
    storage::PageMentionSpot,
    views::{Change, NotifyLevel},
    workspace::{State, Workspace},
};

/// Older mentions, caught up later, stay out of the inbox, so a new device
/// reading the whole history doesn't fill it.
const INBOX_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const ALERTS_PER_AUTHOR: usize = 3;
const ALERT_PERIOD_MS: i64 = 2 * 60 * 1000;

/// When each author's recent page alerts were shown, by our clock.
#[derive(Default)]
pub struct PageAlertTimes(std::collections::HashMap<PublicKey, VecDeque<i64>>);

impl PageAlertTimes {
    /// Takes one of `author`'s alerts for now, if they have one left.
    fn take(&mut self, author: &PublicKey, now: i64) -> bool {
        let times = self.0.entry(*author).or_default();
        while times
            .front()
            .is_some_and(|shown| now.saturating_sub(*shown) >= ALERT_PERIOD_MS)
        {
            times.pop_front();
        }
        if times.len() >= ALERTS_PER_AUTHOR {
            return false;
        }
        times.push_back(now);
        true
    }
}

/// How much of the inserted text around a mention its preview reads, so
/// a huge insert costs no more than a small one.
const PREVIEW_BYTES_BEFORE: usize = 300;
const PREVIEW_BYTES_AFTER: usize = 1_000;

/// A mention of you that an update inserts.
struct FoundMention {
    position: Vec<u8>,
    /// Its line in the inserted text, as markdown, cut to a window around it.
    line: String,
}

impl Workspace {
    /// Call only after `verified` was applied.
    pub(super) fn page_mention_for(&self, state: &State, verified: &VerifiedOp) -> Result<Vec<Change>> {
        let Op::DocUpdate { page, update, .. } = &verified.body.op else {
            return Ok(Vec::new());
        };
        let now = now_ms()?;
        // The author's time can be anything; saturate rather than overflow.
        let age = now.saturating_sub(verified.body.timestamp_ms);
        // Entries, their bounds and alerts go by the person who wrote.
        let author = state.store.person_of(&verified.author)?;
        if author == self.my_person() || age > INBOX_WINDOW_MS || state.store.is_deleted(page)? {
            return Ok(Vec::new());
        }
        let Some(found) = self.last_mention_of_me(update)? else {
            return Ok(Vec::new());
        };
        let names = state
            .store
            .link_names(&self.inner.config.admin, &self.inner.config.workspace_id)?;
        let preview: String = preview_text(&found.line, &names)
            .chars()
            .take(PREVIEW_CHARS)
            .collect();
        let mentioned_ms = verified.body.timestamp_ms.min(now);
        let is_newly_unread = state.store.note_page_mention(
            page,
            author.as_bytes(),
            &found.position,
            &preview,
            mentioned_ms,
        )?;
        let mut changes = Vec::new();
        if is_newly_unread {
            changes.push(Change::PageMentions);
        }
        let is_alerting = age <= ALERT_WINDOW_MS
            && self.status_allows_alerts(state, now)?
            && self
                .inner
                .page_alert_times
                .lock()
                .expect("page alert lock poisoned by an earlier panic")
                .take(&author, now);
        if is_alerting {
            // The inbox entry is saved; a failure here only loses the alert.
            match self.page_alert(state, page, &author, found.position, preview, &names) {
                Ok(alert) => changes.push(alert),
                Err(error) => warn!("couldn't make a page mention's alert: {error:#}"),
            }
        }
        Ok(changes)
    }

    /// Forgets mentions whose text the page no longer holds at all: a
    /// removed member's late edits, undone by a rebuild. Deleted text still
    /// counts; its mention points where it was.
    pub(crate) fn forget_undone_page_mentions(&self, state: &State) -> Result<Vec<Change>> {
        state.store.forget_read_page_mentions()?;
        let mut by_page: BTreeMap<String, Vec<PageMentionSpot>> = BTreeMap::new();
        for spot in state.store.page_mention_positions()? {
            by_page.entry(spot.page.clone()).or_default().push(spot);
        }
        let mut forgotten = false;
        for (page, spots) in by_page {
            let positions: Vec<&[u8]> = spots.iter().map(|spot| spot.position.as_slice()).collect();
            let held = pages::positions_held(state.store.page_state(&page)?.as_deref(), &positions)?;
            for (spot, is_held) in spots.iter().zip(held) {
                if !is_held {
                    state.store.forget_page_mention(&page, &spot.author)?;
                    forgotten = true;
                }
            }
        }
        Ok(if forgotten {
            vec![Change::PageMentions]
        } else {
            Vec::new()
        })
    }

    /// The last mention of this member that `update` inserts.
    fn last_mention_of_me(&self, update: &[u8]) -> Result<Option<FoundMention>> {
        let my_id = self.my_id();
        let workspace_id = &self.inner.config.workspace_id;
        for inserted in pages::insertions(update)?.iter().rev() {
            if let Some(start) = last_member_mention_at(inserted.text, workspace_id, &my_id) {
                return Ok(Some(FoundMention {
                    position: pages::position_in(inserted, start)?,
                    line: line_around(inserted.text, start).to_owned(),
                }));
            }
        }
        Ok(None)
    }

    fn status_allows_alerts(&self, state: &State, now: i64) -> Result<bool> {
        let status = state.store.member_status(self.my_person().as_bytes(), now)?;
        Ok(status.is_none_or(|status| status.alerts != NotifyLevel::Nothing))
    }

    fn page_alert(
        &self,
        state: &State,
        page: &str,
        author: &PublicKey,
        position: Vec<u8>,
        preview: String,
        names: &LinkNames,
    ) -> Result<Change> {
        let author = hex::encode(author.as_bytes());
        let author_name = state
            .store
            .members(&self.inner.config.admin)?
            .into_iter()
            .find(|member| member.id == author)
            .map(|member| member.name)
            .unwrap_or_default();
        Ok(Change::PageAlert {
            page: page.to_owned(),
            page_title: names.pages.get(page).cloned().unwrap_or_default(),
            preview,
            author,
            author_name,
            position,
        })
    }
}

/// The line of `text` around byte `at`, at most a window around it, starting
/// at a word where the window cuts the line.
fn line_around(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |newline| at + newline);
    let mut start = line_start.max(at.saturating_sub(PREVIEW_BYTES_BEFORE));
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if start > line_start {
        start = text[start..at].find(' ').map_or(at, |space| start + space + 1);
    }
    let mut end = line_end.min(at.saturating_add(PREVIEW_BYTES_AFTER));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_reads_the_mention_s_line_within_a_window() {
        assert_eq!(line_around("one\nAsk @Ada now\nthree", 8), "Ask @Ada now");
        let long = format!("{}Ask @Ada{}", "word ".repeat(200), " more".repeat(400));
        let at = long.find("@Ada").unwrap();
        let line = line_around(&long, at);
        assert!(line.starts_with("word ") && line.contains("Ask @Ada"), "{line}");
        assert!(line.len() <= PREVIEW_BYTES_BEFORE + PREVIEW_BYTES_AFTER);
        let wide = format!("{}@Ada{}", "é".repeat(400), "é".repeat(800));
        assert!(line_around(&wide, wide.find('@').unwrap()).starts_with("@Ada"));
    }
}
