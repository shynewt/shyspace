//! Sent: your own messages across the channels you can still read.

use std::collections::HashMap;

use anyhow::{Context, Result};

use super::Workspace;
use crate::views::{LinkedMessage, MessageCursor};

impl Workspace {
    /// Up to `limit` of your own messages, newest first, thread replies
    /// included, each with how people responded. With `before`, only older
    /// ones, to page; with `text`, only those matching it (each word as a
    /// prefix). Private channels you left or were removed from are left out.
    pub fn sent_messages(
        &self,
        text: &str,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<LinkedMessage>> {
        let state = self.state();
        let mut readable = Vec::new();
        for mut channel in state.store.channels()? {
            self.describe_private(&state.store, &mut channel)?;
            if channel.is_member {
                readable.push(channel.id);
            }
        }
        let found = state
            .store
            .my_messages(&self.me(), &readable, text, before, limit)?;
        let order: Vec<String> = found.iter().map(|message| message.id.clone()).collect();
        let mut by_channel: HashMap<String, Vec<_>> = HashMap::new();
        for message in found {
            by_channel
                .entry(message.channel.clone())
                .or_default()
                .push(message);
        }
        let mut seen = HashMap::new();
        for (channel, messages) in by_channel {
            for message in self.as_seen_by_me(&state, &channel, messages)? {
                seen.insert(message.id.clone(), message);
            }
        }
        order
            .into_iter()
            .map(|id| {
                let message = seen.remove(&id).context("sent message vanished")?;
                let thread_root = state.store.thread_root(&id)?.filter(|root| *root != id);
                Ok(LinkedMessage { message, thread_root })
            })
            .collect()
    }
}
