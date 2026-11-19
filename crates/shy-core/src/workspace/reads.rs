//! Read models, straight from the local database, marked with what this
//! member may do to each item.

use anyhow::{Context, Result};

use super::Workspace;
use crate::{
    ops::Op,
    pages,
    views::{Channel, Member, Message, MessageCursor, Page, SearchHit},
};

impl Workspace {
    pub fn channels(&self) -> Result<Vec<Channel>> {
        let state = self.state();
        let mut channels = state.store.channels()?;
        for channel in &mut channels {
            channel.unread = state.store.unread_count(&channel.id, &self.me())?;
            channel.last_read = state.store.read_marker(&channel.id)?;
            channel.can_delete = self.may(&Op::Delete {
                id: channel.id.clone(),
            });
        }
        Ok(channels)
    }

    /// The latest `limit` messages, or with `before`, the ones just older.
    pub fn messages(
        &self,
        channel: &str,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>> {
        let messages = self.state().store.messages(channel, &self.me(), before, limit)?;
        Ok(self.with_permissions(messages))
    }

    /// Up to `limit` messages older than `message`, oldest first.
    pub fn messages_before(&self, channel: &str, message: &str, limit: u32) -> Result<Vec<Message>> {
        let state = self.state();
        let cursor = state.store.message_cursor(message)?.context("no such message")?;
        let messages = state.store.messages(channel, &self.me(), Some(&cursor), limit)?;
        Ok(self.with_permissions(messages))
    }

    fn with_permissions(&self, mut messages: Vec<Message>) -> Vec<Message> {
        for message in &mut messages {
            message.can_edit = self.may(&Op::MessageEdit {
                id: message.id.clone(),
                text: String::new(),
            });
            message.can_delete = self.may(&Op::Delete {
                id: message.id.clone(),
            });
        }
        messages
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut pages = self.state().store.pages()?;
        for page in &mut pages {
            page.can_delete = self.may(&Op::Delete { id: page.id.clone() });
        }
        Ok(pages)
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
