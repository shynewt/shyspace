//! Unread mentions of you in pages, as the inbox shows them.

use anyhow::Result;

use super::{State, Workspace};
use crate::views::{Change, PageMention};

const MAX_PAGE_MENTIONS: usize = 100;

impl Workspace {
    pub(super) fn unread_page_mentions(&self, state: &State) -> Result<Vec<PageMention>> {
        let titles = state.store.pages()?;
        Ok(state
            .store
            .unread_page_mentions(MAX_PAGE_MENTIONS)?
            .into_iter()
            .map(|row| PageMention {
                page_title: titles
                    .iter()
                    .find(|page| page.id == row.page)
                    .map(|page| page.title.clone())
                    .unwrap_or_default(),
                page: row.page,
                author: row.author,
                preview: row.preview,
                position: row.position,
                mentioned_ms: row.mentioned_ms,
            })
            .collect())
    }

    /// Takes `page` out of the inbox: you've seen its mentions of you.
    pub fn mark_page_mentions_read(&self, page: &str) -> Result<()> {
        if self.state().store.mark_page_mentions_read(page)? {
            self.emit(Change::PageMentions);
        }
        Ok(())
    }
}
