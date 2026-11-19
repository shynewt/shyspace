//! The inbox: unread mentions and new replies in threads you follow, across
//! every channel, plus the channels with unread messages.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use super::{State, Workspace, follows_by_default};
use crate::{
    mentions::mentions,
    storage::Reply,
    views::{Change, Channel, Inbox, InboxItem, InboxReason},
};

const MAX_ATTENTION: usize = 100;

impl Workspace {
    pub fn inbox(&self) -> Result<Inbox> {
        let channels = self.channels()?;
        let state = self.state();
        let mut attention = Vec::new();
        for channel in &channels {
            attention.extend(self.attention_in(&state, channel)?);
        }
        attention.sort_by(|left: &InboxItem, right: &InboxItem| {
            (right.message.lamport, &right.message.author).cmp(&(left.message.lamport, &left.message.author))
        });
        attention.truncate(MAX_ATTENTION);
        Ok(Inbox {
            attention,
            channels: channels
                .into_iter()
                .filter(|channel| channel.unread > 0)
                .collect(),
        })
    }

    /// Moves every read position, in channels and threads, to the newest message.
    pub fn mark_all_read(&self) -> Result<()> {
        let channels = {
            let state = self.state();
            let channels = state.store.channels()?;
            for channel in &channels {
                if let Some(latest) = state.store.latest_message(&channel.id)? {
                    state.store.mark_read(&channel.id, &latest)?;
                }
                for (root, replies) in by_thread(state.store.thread_replies(&channel.id)?) {
                    if let Some(last) = replies.last() {
                        state.store.mark_thread_read(&root, &last.id)?;
                    }
                }
            }
            channels
        };
        self.emit(Change::Channels);
        for channel in channels {
            self.emit(Change::Messages { channel: channel.id });
        }
        Ok(())
    }

    fn attention_in(&self, state: &State, channel: &Channel) -> Result<Vec<InboxItem>> {
        let my_id = self.my_id();
        let mut wanted: Vec<(String, Option<String>, InboxReason)> = Vec::new();
        for unread in state.store.unread_messages(&channel.id, &self.me())? {
            if mentions(&unread.text, &my_id) {
                let root = state
                    .store
                    .thread_root(&unread.id)?
                    .filter(|root| *root != unread.id);
                wanted.push((unread.id, root, InboxReason::Mention));
            }
        }
        for (root, replies) in by_thread(state.store.thread_replies(&channel.id)?) {
            if !self.follows(state, &root, &replies)? {
                continue;
            }
            let read = state.store.read_marker(&root)?;
            for reply in replies {
                let is_new = read.as_ref().is_none_or(|read| {
                    (read.lamport, read.author.as_str()) < (reply.lamport, reply.author.as_str())
                });
                let is_listed = wanted.iter().any(|(id, ..)| *id == reply.id);
                if is_new && reply.author != my_id && !is_listed {
                    wanted.push((reply.id, Some(root.clone()), InboxReason::ThreadReply));
                }
            }
        }
        let mut items = Vec::new();
        for (id, thread_root, reason) in wanted {
            let message = state
                .store
                .message(&id, &self.me())?
                .context("inbox message vanished")?;
            let message = self.as_seen_by_me(state, &channel.id, vec![message])?.remove(0);
            items.push(InboxItem {
                message,
                channel_name: channel.name.clone(),
                thread_root,
                reason,
            });
        }
        Ok(items)
    }

    fn follows(&self, state: &State, root: &str, replies: &[Reply]) -> Result<bool> {
        if let Some(choice) = state.store.following_choice(root)? {
            return Ok(choice);
        }
        let first = state
            .store
            .message(root, &self.me())?
            .context("thread without its first message")?;
        Ok(follows_by_default(
            &self.my_id(),
            (&first.author, &first.text),
            replies
                .iter()
                .map(|reply| (reply.author.as_str(), reply.text.as_str())),
        ))
    }
}

fn by_thread(replies: Vec<Reply>) -> BTreeMap<String, Vec<Reply>> {
    let mut threads: BTreeMap<String, Vec<Reply>> = BTreeMap::new();
    for reply in replies {
        threads.entry(reply.root.clone()).or_default().push(reply);
    }
    threads
}
