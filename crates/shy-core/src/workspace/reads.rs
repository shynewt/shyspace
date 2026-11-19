//! Read models, straight from the local database, marked with what this
//! member may do to each item.

use anyhow::{Context, Result};

use super::{State, Workspace};
use crate::{
    mentions::mentions,
    ops::Op,
    pages, search,
    storage::{Reply, SearchFilters},
    views::{Channel, LinkedMessage, Member, Message, MessageCursor, Page, SearchHit},
};

impl Workspace {
    pub fn channels(&self) -> Result<Vec<Channel>> {
        let state = self.state();
        let my_id = self.my_id();
        let mut channels = state.store.channels()?;
        for channel in &mut channels {
            let unread = state.store.unread_messages(&channel.id, &self.me())?;
            // Thread replies count in their thread, but a mention counts anywhere.
            channel.unread = count(unread.iter().filter(|message| message.is_in_channel).count());
            channel.unread_mentions = count(
                unread
                    .iter()
                    .filter(|message| mentions(&message.text, &my_id))
                    .count(),
            );
            channel.last_read = state.store.read_marker(&channel.id)?;
            channel.notify = state.store.notify_level(&channel.id)?;
            channel.can_delete = self.may(&Op::Delete {
                id: channel.id.clone(),
            });
        }
        Ok(channels)
    }

    /// The latest `limit` messages shown in the channel (thread replies
    /// stay in their thread), or with `before`, the ones just older.
    pub fn messages(
        &self,
        channel: &str,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>> {
        let state = self.state();
        let messages = state.store.messages(channel, &self.me(), before, limit)?;
        self.as_seen_by_me(&state, channel, messages)
    }

    /// Up to `limit` messages older than `message`, oldest first.
    pub fn messages_before(&self, channel: &str, message: &str, limit: u32) -> Result<Vec<Message>> {
        let state = self.state();
        let cursor = state.store.message_cursor(message)?.context("no such message")?;
        let messages = state.store.messages(channel, &self.me(), Some(&cursor), limit)?;
        self.as_seen_by_me(&state, channel, messages)
    }

    /// The message a `shy://message/<id>` link points to, if it still exists.
    pub fn linked_message(&self, id: &str) -> Result<Option<LinkedMessage>> {
        let state = self.state();
        let Some(message) = state.store.message(id, &self.me())? else {
            return Ok(None);
        };
        let thread_root = state.store.thread_root(id)?.filter(|root| root != id);
        let channel = message.channel.clone();
        let message = self.as_seen_by_me(&state, &channel, vec![message])?.remove(0);
        Ok(Some(LinkedMessage { message, thread_root }))
    }

    /// The thread starting at `root`: its first message, then every reply.
    pub fn thread(&self, root: &str) -> Result<Vec<Message>> {
        let state = self.state();
        let messages = state.store.thread(root, &self.me())?;
        let channel = messages.first().context("no such thread")?.channel.clone();
        self.as_seen_by_me(&state, &channel, messages)
    }

    /// Marks what this member may do with each message, whether it mentions
    /// them, and for a thread's first message, its replies and follow state.
    pub(super) fn as_seen_by_me(
        &self,
        state: &State,
        channel: &str,
        mut messages: Vec<Message>,
    ) -> Result<Vec<Message>> {
        let my_id = self.my_id();
        let replies = state.store.thread_replies(channel)?;
        for message in &mut messages {
            message.mentions_me = message.author != my_id && mentions(&message.text, &my_id);
            message.can_edit = self.may(&Op::MessageEdit {
                id: message.id.clone(),
                text: String::new(),
            });
            message.can_delete = self.may(&Op::Delete {
                id: message.id.clone(),
            });
            let thread: Vec<&Reply> = replies.iter().filter(|reply| reply.root == message.id).collect();
            message.reply_count = count(thread.len());
            message.last_reply_ms = thread.iter().map(|reply| reply.created_ms).max();
            message.is_following = match state.store.following_choice(&message.id)? {
                Some(choice) => choice,
                None => follows_by_default(
                    &my_id,
                    (&message.author, &message.text),
                    thread
                        .iter()
                        .map(|reply| (reply.author.as_str(), reply.text.as_str())),
                ),
            };
            if message.is_following && !thread.is_empty() {
                let read = state.store.read_marker(&message.id)?;
                message.unread_replies = count(
                    thread
                        .iter()
                        .filter(|reply| {
                            reply.author != my_id
                                && read.as_ref().is_none_or(|read| {
                                    (read.lamport, read.author.as_str())
                                        < (reply.lamport, reply.author.as_str())
                                })
                        })
                        .count(),
                );
            }
        }
        Ok(messages)
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

    /// Searches messages and pages; see `search::parse` for the filters.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let query = search::parse(query)?;
        let state = self.state();
        let mut filters = SearchFilters {
            before_ms: query.before_ms,
            after_ms: query.after_ms,
            ..SearchFilters::default()
        };
        if let Some(from) = &query.from {
            filters.authors = Some(if from.eq_ignore_ascii_case("me") {
                vec![self.me().as_bytes().to_vec()]
            } else {
                let wanted = from.to_lowercase();
                state
                    .store
                    .members(&self.inner.config.admin)?
                    .into_iter()
                    .filter(|member| member.name.to_lowercase().starts_with(&wanted))
                    .map(|member| hex::decode(&member.id))
                    .collect::<Result<_, _>>()?
            });
        }
        if let Some(channel) = &query.channel {
            let wanted = channel.to_lowercase();
            let Some(found) = state
                .store
                .channels()?
                .into_iter()
                .find(|c| c.name.to_lowercase() == wanted)
            else {
                return Ok(Vec::new());
            };
            filters.channel = Some(found.id);
        }
        let mut hits = state.store.search(&query.text, &filters, limit)?;
        for hit in &mut hits {
            if hit.kind == "message" {
                hit.thread_root = state.store.thread_root(&hit.id)?.filter(|root| *root != hit.id);
            }
        }
        Ok(hits)
    }
}

/// Without an explicit choice, you follow a thread you started, replied in,
/// or were mentioned in.
pub(crate) fn follows_by_default<'a>(
    my_id: &str,
    (root_author, root_text): (&str, &str),
    mut replies: impl Iterator<Item = (&'a str, &'a str)>,
) -> bool {
    root_author == my_id
        || mentions(root_text, my_id)
        || replies.any(|(author, text)| author == my_id || mentions(text, my_id))
}

fn count(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}
