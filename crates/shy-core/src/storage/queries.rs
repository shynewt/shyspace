//! Read models for the UI and agents.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::views::{
    Channel, ChannelKind, Member, Message, MessageCursor, NotifyLevel, Page, PresenceState, Reaction,
    SearchHit,
};

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

/// SQL for the person whose device the column `$device` names: the device
/// itself, unless the counted device map links it to someone. A device
/// writes only from the epoch it was linked at, so this is its person when
/// it wrote.
macro_rules! person_of {
    ($device:literal) => {
        concat!(
            "COALESCE((SELECT d.person FROM devices d WHERE d.device = ",
            $device,
            "), ",
            $device,
            ")"
        )
    };
}

/// The person who wrote message `m`.
pub(super) const MESSAGE_PERSON: &str = person_of!("m.author");
/// The columns `message_from_row` reads, and the joins they need.
pub(super) const MESSAGE_COLUMNS: &str = concat!(
    "m.id, m.channel, ",
    person_of!("m.author"),
    " AS person, COALESCE(p.name, ''), COALESCE(p.is_bot, 0),
     COALESCE(e.text, m.text), m.reply_to, m.created_ms, m.lamport, e.message IS NOT NULL, m.author"
);
/// Whether message `m` shows in its channel rather than in a thread: it
/// isn't a reply, or its parent hasn't arrived or sits in another channel
/// (threads stay within a channel).
pub(super) const IS_SHOWN_IN_CHANNEL: &str = "(m.reply_to IS NULL OR NOT EXISTS (
    SELECT 1 FROM messages parent WHERE parent.id = m.reply_to AND parent.channel = m.channel))";
pub(super) const MESSAGE_JOINS: &str = concat!(
    "FROM messages m
     LEFT JOIN profiles p ON p.rowid = (
         SELECT rowid FROM profiles
         WHERE member = ",
    person_of!("m.author"),
    "
            OR member IN (SELECT device FROM devices WHERE person = ",
    person_of!("m.author"),
    ")
         ORDER BY lamport DESC, member DESC LIMIT 1)
     LEFT JOIN message_edits e ON e.message = m.id"
);

/// A message as stored; what this member may do with it is filled in later.
pub(super) fn message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        channel: row.get(1)?,
        author: hex::encode(row.get::<_, Vec<u8>>(2)?),
        device: hex::encode(row.get::<_, Vec<u8>>(10)?),
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
        saved: None,
        reply_count: 0,
        last_reply_ms: None,
        recent_reply_authors: Vec::new(),
        last_reply_author: None,
        last_reply_preview: None,
        is_following: false,
        unread_replies: 0,
        reactions: Vec::new(),
        files: Vec::new(),
    })
}

impl Store {
    pub fn page_state(&self, id: &str) -> Result<Option<Vec<u8>>> {
        match self.pending_page_state(id) {
            Some(state) => Ok(Some(state)),
            None => self.stored_page_state(id),
        }
    }

    pub(super) fn stored_page_state(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT doc FROM pages WHERE id = ?", [id], |row| row.get(0))
            .optional()?
            .flatten())
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        let mut statement = self.db.prepare(
            "SELECT c.id, COALESCE(n.name, c.name) FROM channels c
                 LEFT JOIN channel_names n ON n.channel = c.id ORDER BY c.lamport, c.author, c.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Channel {
                id: row.get(0)?,
                kind: ChannelKind::Public,
                name: row.get(1)?,
                dm_members: Vec::new(),
                is_member: true,
                is_missing_key: false,
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
            Some(cursor) => (Some(cursor.lamport), Some(hex::decode(&cursor.device)?)),
            None => (None, None),
        };
        let mut statement = self.db.prepare(&format!(
            "SELECT * FROM (
                    SELECT {MESSAGE_COLUMNS} {MESSAGE_JOINS}
                    WHERE m.channel = ?1
                      AND {IS_SHOWN_IN_CHANNEL}
                      AND (?2 IS NULL OR m.lamport < ?2 OR (m.lamport = ?2 AND m.author < ?3))
                    ORDER BY m.lamport DESC, m.author DESC LIMIT ?4)
                 ORDER BY lamport, author"
        ))?;
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
        Ok(row.map(|(lamport, device)| MessageCursor {
            lamport,
            device: hex::encode(device),
        }))
    }

    fn reactions(&self, message: &str, me: &PublicKey) -> Result<Vec<Reaction>> {
        // Per emoji and person, the newest toggle among their devices decides
        // (by clock, then device key), so any of them can take a reaction
        // back and a person counts once.
        let mut statement = self.db.prepare(concat!(
            "WITH toggles AS (
                 SELECT emoji, active, rowid AS position, ",
            person_of!("r.author"),
            " AS person, lamport, author
                 FROM reactions r WHERE message = ?1),
             latest AS (
                 SELECT emoji, active, position, person,
                        ROW_NUMBER() OVER (PARTITION BY emoji, person ORDER BY lamport DESC, author DESC) AS rank
                 FROM toggles)
             SELECT emoji, COUNT(*), MAX(person = ?2), MIN(position) AS first FROM latest
             WHERE rank = 1 AND active = 1 GROUP BY emoji ORDER BY first"
        ))?;
        let rows = statement.query_map(params![message, me.as_bytes()], |row| {
            Ok(Reaction {
                emoji: row.get(0)?,
                count: row.get(1)?,
                mine: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many of `device`'s own reactions to `message` are on in its log,
    /// leaving out `emoji`: what its limit counts.
    pub fn own_reactions_on(&self, message: &str, device: &PublicKey, emoji: &str) -> Result<usize> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM reactions WHERE message = ?1 AND author = ?2 AND emoji != ?3 AND active = 1",
            params![message, device.as_bytes(), emoji],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count)?)
    }

    /// Of `device`'s reactions to `message` that are on in its own log but
    /// that its person shows off (another of their devices took them back),
    /// the one taken back longest ago: the least likely to have been added
    /// again somewhere this device hasn't heard of yet.
    pub fn oldest_own_reaction_shown_off(&self, message: &str, device: &PublicKey) -> Result<Option<String>> {
        let person = self.person_of(device)?;
        let mut own = self
            .db
            .prepare("SELECT emoji FROM reactions WHERE message = ?1 AND author = ?2 AND active = 1")?;
        let emojis = own
            .query_map(params![message, device.as_bytes()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut oldest: Option<(u64, String)> = None;
        for emoji in emojis {
            let (is_on, lamport): (bool, u64) = self.db.query_row(
                "SELECT active, lamport FROM reactions
                 WHERE message = ?1 AND emoji = ?2
                   AND (author = ?3 OR author IN (SELECT device FROM devices WHERE person = ?3))
                 ORDER BY lamport DESC, author DESC LIMIT 1",
                params![message, emoji, person.as_bytes()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let candidate = (lamport, emoji);
            if !is_on && oldest.as_ref().is_none_or(|kept| candidate < *kept) {
                oldest = Some(candidate);
            }
        }
        Ok(oldest.map(|(_, emoji)| emoji))
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut statement = self.db.prepare(
            // A sub-page of a deleted page shows at the top level.
            "SELECT id, CASE WHEN parent IN (SELECT id FROM deleted) THEN NULL ELSE parent END,
                    title, updated_ms
             FROM pages WHERE created_ms IS NOT NULL ORDER BY create_lamport, create_author, id",
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
            // One row per person, active while any of their devices is, with
            // the newest profile among their devices. Until a newcomer's own
            // profile arrives, the admin knows them by the name in their
            // approved join request.
            concat!(
                "WITH people AS (
                 SELECT ",
                person_of!("m.member"),
                " AS person,
                        MAX(m.active) AS active
                 FROM members m GROUP BY 1)
             SELECT people.person,
                    COALESCE(p.name, i.joiner_name, ''),
                    COALESCE(p.is_bot, i.joiner_is_bot, 0),
                    people.active
             FROM people
             LEFT JOIN profiles p ON p.rowid = (
                 SELECT rowid FROM profiles
                 WHERE member = people.person
                    OR member IN (SELECT device FROM devices WHERE person = people.person)
                 ORDER BY lamport DESC, member DESC LIMIT 1)
             LEFT JOIN invites i ON i.joiner = people.person AND i.state = 'accepted'
             ORDER BY people.active DESC, COALESCE(p.name, i.joiner_name)"
            ),
        )?;
        let rows = statement.query_map([], |row| {
            let id: Vec<u8> = row.get(0)?;
            Ok(Member {
                is_admin: id == admin.as_bytes(),
                id: hex::encode(id),
                name: row.get(1)?,
                is_bot: row.get(2)?,
                active: row.get(3)?,
                status: None,
                presence: PresenceState::default(),
                details: crate::profile::ProfileDetails::default(),
                picture: None,
                safety_code: String::new(),
            })
        })?;
        let mut members: Vec<Member> = rows.collect::<Result<_, _>>()?;
        for member in &mut members {
            member.safety_code = crate::views::safety_code(&crate::views::parse_member_id(&member.id)?);
        }
        Ok(members)
    }

    /// Messages and pages matching `text` (each word as a prefix). With
    /// `filters`, only messages that also pass them; with no text, the
    /// newest messages that pass them. Snippets are left empty: they are
    /// made from what the reader sees (`search::snippet`).
    pub fn search(&self, text: &str, filters: &SearchFilters, limit: u32) -> Result<Vec<SearchHit>> {
        let match_expression = match_expression(text);
        if match_expression.is_empty() && filters.is_empty() {
            return Ok(Vec::new());
        }
        let mut conditions = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(authors) = &filters.authors {
            if authors.is_empty() {
                return Ok(Vec::new());
            }
            conditions.push(format!(
                "{MESSAGE_PERSON} IN ({})",
                vec!["?"; authors.len()].join(", ")
            ));
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
                "SELECT 'message', m.id, COALESCE(n.name, c.name, ''), '', m.channel,
                        {MESSAGE_PERSON}, m.created_ms
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
                        '', m.channel,
                        {MESSAGE_PERSON}, COALESCE(m.created_ms, p.updated_ms)
                 FROM search s
                 LEFT JOIN messages m ON s.kind = 'message' AND m.id = s.ref
                 LEFT JOIN pages p ON s.kind = 'page' AND p.id = s.ref
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
                author: row.get::<_, Option<Vec<u8>>>(5)?.map(hex::encode),
                time_ms: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// A message's text as it stands, edits included.
    pub(crate) fn message_text(&self, message: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT COALESCE(e.text, m.text) FROM messages m
                 LEFT JOIN message_edits e ON e.message = m.id WHERE m.id = ?",
                [message],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Everyone's name and every page's title, for naming app links.
    pub(crate) fn link_names(
        &self,
        admin: &PublicKey,
        workspace_id: &str,
    ) -> Result<crate::markdown::LinkNames> {
        Ok(crate::markdown::LinkNames {
            workspace_id: workspace_id.to_owned(),
            members: self
                .members(admin)?
                .into_iter()
                .map(|member| (member.id, member.name))
                .collect(),
            pages: self
                .pages()?
                .into_iter()
                .map(|page| (page.id, page.title))
                .collect(),
        })
    }

    /// A person's status, the newest any of their devices set, if set and
    /// not expired at `now_ms`. `member` may be any of their devices.
    pub fn member_status(&self, member: &[u8], now_ms: i64) -> Result<Option<crate::views::MemberStatus>> {
        let person = self.person_of(&super::to_public_key(member.to_vec())?)?;
        let person = person.as_bytes();
        let row: Option<(String, String, Option<i64>, u8)> = self
            .db
            .query_row(
                "SELECT emoji, text, until_ms, alerts FROM member_status
                 WHERE member = ?1 OR member IN (SELECT device FROM devices WHERE person = ?1)
                 ORDER BY lamport DESC, member DESC LIMIT 1",
                [person],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((emoji, text, until_ms, alerts)) = row else {
            return Ok(None);
        };
        let alerts = crate::views::NotifyLevel::from(crate::ops::StatusAlerts::try_from(alerts)?);
        let is_set = !emoji.is_empty() || !text.is_empty() || alerts != crate::views::NotifyLevel::All;
        let is_current = until_ms.is_none_or(|until| until > now_ms);
        Ok((is_set && is_current).then_some(crate::views::MemberStatus {
            emoji,
            text,
            until_ms,
            alerts,
        }))
    }
}

/// `text` as an FTS5 query: each word matches as a prefix, quotes are plain text.
pub(super) fn match_expression(text: &str) -> String {
    text.split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
