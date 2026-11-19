//! Read models for the UI and agents.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::{Channel, Member, Message, MessageCursor, NotifyLevel, Page, Reaction, SearchHit};

/// Message filters for `Store::search`, with names already resolved.
#[derive(Default)]
pub struct SearchFilters {
    /// Only these authors (keys); an empty list matches nothing.
    pub authors: Option<Vec<Vec<u8>>>,
    pub channel: Option<String>,
    pub before_ms: Option<i64>,
    pub after_ms: Option<i64>,
}

impl SearchFilters {
    fn is_empty(&self) -> bool {
        self.authors.is_none()
            && self.channel.is_none()
            && self.before_ms.is_none()
            && self.after_ms.is_none()
    }
}

/// The columns `message_from_row` reads, and the joins they need.
pub(super) const MESSAGE_COLUMNS: &str =
    "m.id, m.channel, m.author, COALESCE(p.name, ''), COALESCE(p.is_bot, 0),
     COALESCE(e.text, m.text), m.reply_to, m.created_ms, m.lamport, e.message IS NOT NULL";
pub(super) const MESSAGE_JOINS: &str = "FROM messages m
     LEFT JOIN profiles p ON p.member = m.author
     LEFT JOIN message_edits e ON e.message = m.id";

/// A message as stored; what this member may do with it is filled in later.
pub(super) fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        channel: row.get(1)?,
        author: hex::encode(row.get::<_, Vec<u8>>(2)?),
        author_name: row.get(3)?,
        author_is_bot: row.get(4)?,
        text: row.get(5)?,
        reply_to: row.get(6)?,
        created_ms: row.get(7)?,
        lamport: row.get(8)?,
        is_edited: row.get(9)?,
        can_edit: false,
        can_delete: false,
        mentions_me: false,
        is_saved: false,
        reply_count: 0,
        last_reply_ms: None,
        is_following: false,
        unread_replies: 0,
        reactions: Vec::new(),
    })
}

impl Store {
    pub fn page_state(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT doc FROM pages WHERE id = ?", [id], |row| row.get(0))
            .optional()?
            .flatten())
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        let mut statement = self.db.prepare(
            "SELECT c.id, COALESCE(n.name, c.name) FROM channels c
                 LEFT JOIN channel_names n ON n.channel = c.id ORDER BY c.created_ms, c.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Channel {
                id: row.get(0)?,
                name: row.get(1)?,
                can_delete: false,
                unread: 0,
                unread_mentions: 0,
                last_read: None,
                notify: NotifyLevel::default(),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Up to `limit` messages in `channel`, oldest first. With `before`, only
    /// messages older than that one, for loading history page by page.
    pub fn messages(
        &self,
        channel: &str,
        me: &PublicKey,
        before: Option<&MessageCursor>,
        limit: u32,
    ) -> Result<Vec<Message>> {
        let (before_lamport, before_author) = match before {
            Some(cursor) => (Some(cursor.lamport), Some(hex::decode(&cursor.author)?)),
            None => (None, None),
        };
        let mut statement = self.db.prepare(
            &format!(
                "SELECT * FROM (
                    SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS}
                    WHERE m.channel = ?1
                      AND (m.reply_to IS NULL OR NOT EXISTS (SELECT 1 FROM messages parent WHERE parent.id = m.reply_to))
                      AND (?2 IS NULL OR m.lamport < ?2 OR (m.lamport = ?2 AND m.author < ?3))
                    ORDER BY m.lamport DESC, m.author DESC LIMIT ?4)
                 ORDER BY lamport, author"
            ),
        )?;
        let rows = statement.query_map(
            params![channel, before_lamport, before_author, limit],
            message_from_row,
        )?;
        self.with_reactions(rows.collect::<Result<_, _>>()?, me)
    }

    pub(super) fn with_reactions(&self, mut messages: Vec<Message>, me: &PublicKey) -> Result<Vec<Message>> {
        for message in &mut messages {
            message.reactions = self.reactions(&message.id, me)?;
        }
        Ok(messages)
    }

    /// Where a message sits in its channel's order, to page from it.
    pub fn message_cursor(&self, id: &str) -> Result<Option<MessageCursor>> {
        let row: Option<(u64, Vec<u8>)> = self
            .db
            .query_row("SELECT lamport, author FROM messages WHERE id = ?", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        Ok(row.map(|(lamport, author)| MessageCursor {
            lamport,
            author: hex::encode(author),
        }))
    }

    fn reactions(&self, message: &str, me: &PublicKey) -> Result<Vec<Reaction>> {
        let mut statement = self.db.prepare(
            "SELECT emoji, COUNT(*), MAX(author = ?2), MIN(rowid) AS first FROM reactions
             WHERE message = ?1 AND active = 1 GROUP BY emoji ORDER BY first",
        )?;
        let rows = statement.query_map(params![message, me.as_bytes()], |row| {
            Ok(Reaction {
                emoji: row.get(0)?,
                count: row.get(1)?,
                mine: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut statement = self.db.prepare(
            // A sub-page of a deleted page shows at the top level.
            "SELECT id, CASE WHEN parent IN (SELECT id FROM deleted) THEN NULL ELSE parent END,
                    title, updated_ms
             FROM pages WHERE created_ms IS NOT NULL ORDER BY created_ms, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Page {
                id: row.get(0)?,
                parent: row.get(1)?,
                title: row.get(2)?,
                updated_ms: row.get(3)?,
                can_delete: false,
                is_saved: false,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn members(&self, admin: &PublicKey) -> Result<Vec<Member>> {
        let mut statement = self.db.prepare(
            // Until a newcomer's own profile arrives, the admin knows them by the
            // name in their approved join request.
            "SELECT m.member,
                    COALESCE(p.name, i.joiner_name, ''),
                    COALESCE(p.is_bot, i.joiner_is_bot, 0),
                    m.active
             FROM members m
             LEFT JOIN profiles p ON p.member = m.member
             LEFT JOIN invites i ON i.joiner = m.member AND i.state = 'accepted'
             ORDER BY m.active DESC, COALESCE(p.name, i.joiner_name)",
        )?;
        let rows = statement.query_map([], |row| {
            let id: Vec<u8> = row.get(0)?;
            Ok(Member {
                is_admin: id == admin.as_bytes(),
                id: hex::encode(id),
                name: row.get(1)?,
                is_bot: row.get(2)?,
                active: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Messages and pages matching `text` (each word as a prefix). With
    /// `filters`, only messages that also pass them; with no text, the
    /// newest messages that pass them.
    pub fn search(&self, text: &str, filters: &SearchFilters, limit: u32) -> Result<Vec<SearchHit>> {
        let match_expression = text
            .split_whitespace()
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if match_expression.is_empty() && filters.is_empty() {
            return Ok(Vec::new());
        }
        let mut conditions = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(authors) = &filters.authors {
            if authors.is_empty() {
                return Ok(Vec::new());
            }
            conditions.push(format!("m.author IN ({})", vec!["?"; authors.len()].join(", ")));
            values.extend(
                authors
                    .iter()
                    .map(|author| Box::new(author.clone()) as Box<dyn rusqlite::ToSql>),
            );
        }
        if let Some(channel) = &filters.channel {
            conditions.push("m.channel = ?".into());
            values.push(Box::new(channel.clone()));
        }
        if let Some(before) = filters.before_ms {
            conditions.push("m.created_ms < ?".into());
            values.push(Box::new(before));
        }
        if let Some(after) = filters.after_ms {
            conditions.push("m.created_ms >= ?".into());
            values.push(Box::new(after));
        }
        let filter_sql = conditions
            .iter()
            .map(|condition| format!(" AND {condition}"))
            .collect::<String>();
        let sql = if match_expression.is_empty() {
            format!(
                "SELECT 'message', m.id, COALESCE(n.name, c.name, ''), COALESCE(e.text, m.text), m.channel
                 FROM messages m
                 LEFT JOIN message_edits e ON e.message = m.id
                 LEFT JOIN channels c ON c.id = m.channel
                 LEFT JOIN channel_names n ON n.channel = m.channel
                 WHERE 1{filter_sql} ORDER BY m.lamport DESC LIMIT ?"
            )
        } else {
            let only_messages = if filters.is_empty() {
                ""
            } else {
                " AND s.kind = 'message'"
            };
            values.insert(0, Box::new(match_expression));
            format!(
                "SELECT s.kind, s.ref, COALESCE(NULLIF(s.title, ''), n.name, c.name, ''),
                        snippet(search, 3, '\u{1}', '\u{2}', '…', 12), m.channel
                 FROM search s
                 LEFT JOIN messages m ON s.kind = 'message' AND m.id = s.ref
                 LEFT JOIN channels c ON c.id = m.channel
                 LEFT JOIN channel_names n ON n.channel = m.channel
                 WHERE search MATCH ?{only_messages}{filter_sql} ORDER BY rank LIMIT ?"
            )
        };
        values.push(Box::new(limit));
        let mut statement = self.db.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values.iter()), |row| {
            Ok(SearchHit {
                kind: row.get(0)?,
                id: row.get(1)?,
                title: row.get(2)?,
                snippet: row.get(3)?,
                channel: row.get(4)?,
                thread_root: None,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
