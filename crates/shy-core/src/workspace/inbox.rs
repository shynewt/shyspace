//! The inbox: unread mentions and new replies in threads you follow, across
//! every channel, plus the channels with unread messages.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};

use super::{State, Workspace, follows_by_default};
use crate::{
    storage::Reply,
    views::{Change, Channel, Inbox, InboxItem, InboxReason},
};

const MAX_ATTENTION: usize = 100;

/// A message the inbox may show, before loading it.
struct Wanted {
    id: String,
    thread_root: Option<String>,
    reason: InboxReason,
    lamport: u64,
    /// The author's member id (hex), which breaks ties in order.
    author: String,
    channel: String,
    channel_name: String,
}

impl Workspace {
    pub fn inbox(&self) -> Result<Inbox> {
        let channels = self.channels()?;
        let state = self.state();
        let mut wanted = Vec::new();
        for channel in &channels {
            wanted.extend(self.attention_in(&state, channel)?);
        }
        wanted.sort_by(|left: &Wanted, right: &Wanted| {
            (right.lamport, &right.author).cmp(&(left.lamport, &left.author))
        });
        wanted.truncate(MAX_ATTENTION);
        let attention = wanted
            .into_iter()
            .map(|wanted| self.inbox_item(&state, wanted))
            .collect::<Result<_>>()?;
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

    /// What in `channel` needs attention, not yet loaded: the inbox shows
    /// only the newest of it across every channel.
    fn attention_in(&self, state: &State, channel: &Channel) -> Result<Vec<Wanted>> {
        let my_id = self.my_id();
        let mut wanted = Vec::new();
        for mention in
            state
                .store
                .unread_mentions(&self.inner.config.workspace_id, &channel.id, &self.me())?
        {
            let thread_root = state
                .store
                .thread_root(&mention.id)?
                .filter(|root| *root != mention.id);
            wanted.push(Wanted {
                id: mention.id,
                thread_root,
                reason: InboxReason::Mention,
                lamport: mention.lamport,
                author: mention.author,
                channel: channel.id.clone(),
                channel_name: channel.name.clone(),
            });
        }
        let mentioned: HashSet<String> = wanted.iter().map(|wanted| wanted.id.clone()).collect();
        for (root, replies) in by_thread(state.store.thread_replies(&channel.id)?) {
            if !self.follows(state, &root, &replies)? {
                continue;
            }
            let read = state.store.read_marker(&root)?;
            for reply in replies {
                let is_new = read.as_ref().is_none_or(|read| {
                    (read.lamport, read.author.as_str()) < (reply.lamport, reply.author.as_str())
                });
                if is_new && reply.author != my_id && !mentioned.contains(&reply.id) {
                    wanted.push(Wanted {
                        id: reply.id,
                        thread_root: Some(root.clone()),
                        reason: InboxReason::ThreadReply,
                        lamport: reply.lamport,
                        author: reply.author,
                        channel: channel.id.clone(),
                        channel_name: channel.name.clone(),
                    });
                }
            }
        }
        Ok(wanted)
    }

    fn inbox_item(&self, state: &State, wanted: Wanted) -> Result<InboxItem> {
        let message = state
            .store
            .message(&wanted.id, &self.me())?
            .context("inbox message vanished")?;
        let message = self
            .as_seen_by_me(state, &wanted.channel, vec![message])?
            .remove(0);
        Ok(InboxItem {
            message,
            channel_name: wanted.channel_name,
            thread_root: wanted.thread_root,
            reason: wanted.reason,
        })
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
            &self.inner.config.workspace_id,
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
