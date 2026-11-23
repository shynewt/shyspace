//! Read models, straight from the local database, marked with what this
//! member may do to each item.

use std::collections::HashMap;

use anyhow::{Context, Result};

use super::{State, Workspace};
use crate::{
    mentions::{mentions, with_plain_mentions},
    ops::{Op, now_ms},
    pages, search,
    storage::{Reply, SearchFilters},
    views::{
        Channel, ChannelKind, LinkedMessage, Member, Message, MessageCursor, NotifyLevel, Page, SearchHit,
        WorkspaceBadge,
    },
};

impl Workspace {
    /// What this workspace's rail icon shows, counted as its sidebar and
    /// the window badge do.
    pub fn badge(&self) -> Result<WorkspaceBadge> {
        let channels = self.channels()?;
        let is_dm = |channel: &str| {
            channels
                .iter()
                .any(|candidate| candidate.id == channel && candidate.kind == ChannelKind::Dm)
        };
        let outside_dms = self
            .inbox()?
            .attention
            .iter()
            .filter(|item| !is_dm(&item.message.channel))
            .count();
        let now = now_ms()?;
        let due = self
            .later()?
            .iter()
            .filter(|item| !item.is_done && item.remind_ms.is_some_and(|remind_ms| remind_ms <= now))
            .count();
        let mut badge = WorkspaceBadge {
            workspace_id: self.workspace_id().to_owned(),
            unread: false,
            mentions: 0,
            attention: u32::try_from(outside_dms.saturating_add(due)).unwrap_or(u32::MAX),
        };
        for channel in channels {
            if channel.kind == ChannelKind::Dm && channel.notify != NotifyLevel::Nothing {
                badge.attention = badge.attention.saturating_add(channel.unread);
            }
            let is_muted = channel.notify == NotifyLevel::Nothing;
            let mentions = match (channel.kind, is_muted) {
                (_, true) => 0,
                (ChannelKind::Dm, false) => channel.unread,
                (_, false) => channel.unread_mentions,
            };
            badge.mentions = badge.mentions.saturating_add(mentions);
            badge.unread |= mentions > 0 || (!is_muted && channel.unread > 0);
        }
        Ok(badge)
    }

    /// A place's name as people see it: "#design", or a DM's people
    /// (other than you) by name.
    pub fn place_name(&self, channel: &str) -> Result<String> {
        let place = self
            .channels()?
            .into_iter()
            .find(|candidate| candidate.id == channel)
            .context("no such channel here")?;
        if place.kind != ChannelKind::Dm {
            return Ok(format!("#{}", place.name));
        }
        let members = self.members()?;
        let names: Vec<String> = place
            .dm_members
            .iter()
            .map(|id| {
                members
                    .iter()
                    .find(|member| member.id == *id)
                    .map_or_else(|| id.clone(), |member| member.name.clone())
            })
            .collect();
        Ok(names.join(", "))
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        let state = self.state();
        let mut channels = state.store.channels()?;
        for channel in &mut channels {
            // Thread replies count in their thread, but a mention counts anywhere.
            let unread = state.store.unread_count(&channel.id, &self.me())?;
            channel.unread = u32::try_from(unread).unwrap_or(u32::MAX);
            channel.unread_mentions = count(state.store.unread_mentions(&channel.id, &self.me())?.len());
            channel.last_read = state.store.read_marker(&channel.id)?;
            channel.notify = state.store.notify_level(&channel.id)?;
            channel.can_delete = self.may(&Op::Delete {
                id: channel.id.clone(),
            });
            self.describe_private(&state.store, channel)?;
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

    /// The newest message in `channel`, thread replies included.
    pub fn latest_message(&self, channel: &str) -> Result<Option<LinkedMessage>> {
        let latest = self.state().store.latest_message(channel)?;
        match latest {
            Some(id) => self.linked_message(&id),
            None => Ok(None),
        }
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
        let roots: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
        let replies = state.store.replies_in_threads(channel, &roots)?;
        let mut replies_by_root: HashMap<&str, Vec<&Reply>> = HashMap::new();
        for reply in &replies {
            replies_by_root
                .entry(reply.root.as_str())
                .or_default()
                .push(reply);
        }
        for message in &mut messages {
            message.mentions_me = message.author != my_id && mentions(&message.text, &my_id);
            message.saved = state.store.saved_mark(&message.id)?;
            message.can_edit = self.may(&Op::MessageEdit {
                id: message.id.clone(),
                text: String::new(),
            });
            message.can_delete = self.may_delete(&state.store, &message.id)?;
            message.files = state
                .store
                .files_of_message(&message.id, &message.channel)?
                .into_iter()
                .map(|file| self.file_info(&state.store, file))
                .collect::<Result<_>>()?;
            let thread = replies_by_root.remove(message.id.as_str()).unwrap_or_default();
            message.reply_count = count(thread.len());
            message.last_reply_ms = thread.iter().map(|reply| reply.created_ms).max();
            message.recent_reply_authors = recent_authors(&thread);
            message.last_reply_author = thread.last().map(|reply| reply.author.clone());
            message.last_reply_preview = thread.last().map(|reply| reply_preview(&reply.text));
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
        let state = self.state();
        let saved = state.store.saved_items()?;
        let mut pages = state.store.pages()?;
        for page in &mut pages {
            page.can_delete = self.may(&Op::Delete { id: page.id.clone() });
            page.is_saved = saved.contains(&page.id);
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
        let now = now_ms()?;
        let mut members = self.state().store.members(&self.inner.config.admin)?;
        for member in &mut members {
            let key = crate::views::parse_member_id(&member.id)?;
            member.status = self.state().store.member_status(key.as_bytes(), now)?;
            member.presence = self.presence_of(&key, now);
            member.details = self.state().store.profile_details(key.as_bytes())?;
            member.picture = self
                .state()
                .store
                .profile_picture(key.as_bytes())?
                .map(|picture| picture.id);
        }
        Ok(members)
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

const RECENT_REPLY_AUTHORS: usize = 3;
const REPLY_PREVIEW_CHARS: usize = 100;

/// The last few distinct authors in `thread` (which runs oldest first), newest first.
fn recent_authors(thread: &[&Reply]) -> Vec<String> {
    let mut authors: Vec<String> = Vec::new();
    for reply in thread.iter().rev() {
        if authors.len() == RECENT_REPLY_AUTHORS {
            break;
        }
        if !authors.contains(&reply.author) {
            authors.push(reply.author.clone());
        }
    }
    authors
}

/// `text` on one line with mentions as @name, cut on a character boundary.
fn reply_preview(text: &str) -> String {
    let line = with_plain_mentions(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if line.chars().count() <= REPLY_PREVIEW_CHARS {
        return line;
    }
    let mut preview: String = line.chars().take(REPLY_PREVIEW_CHARS - 1).collect();
    preview.truncate(preview.trim_end().len());
    preview.push('…');
    preview
}
