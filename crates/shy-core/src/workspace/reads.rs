//! Read models, straight from the local database.

use anyhow::Result;

use super::Workspace;
use crate::{
    pages,
    views::{Channel, Member, Message, MessageCursor, Page, SearchHit},
};

impl Workspace {
    pub fn channels(&self) -> Result<Vec<Channel>> {
        self.state().store.channels()
    }

    /// The latest `limit` messages, or with `before`, the ones just older.
    pub fn messages(
        &self,
        channel: &str,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>> {
        self.state().store.messages(channel, &self.me(), before, limit)
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        self.state().store.pages()
    }

    pub fn page_state(&self, page: &str) -> Result<Option<Vec<u8>>> {
        self.state().store.page_state(page)
    }

    pub fn page_text(&self, page: &str) -> Result<String> {
        pages::read(self.page_state(page)?.as_deref())
    }

    pub fn members(&self) -> Result<Vec<Member>> {
        self.state().store.members(&self.inner.config.admin)
    }

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        self.state().store.search(query, limit)
    }
}
