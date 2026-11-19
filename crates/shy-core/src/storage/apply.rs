//! Folds verified ops into the views. Every op merges the same way whatever
//! order ops arrive in, and applying one twice changes nothing.

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, named_params, params};

use super::Store;
use crate::{
    config,
    ops::{Op, VerifiedOp, is_owned_by},
    pages,
    views::Change,
};

/// A creation that conflicts with an existing id keeps the earliest one by
/// (lamport, author), so every replica picks the same winner.
const EARLIER_WINS: &str = "(:lamport < lamport OR (:lamport = lamport AND :author < author))";

impl Store {
    /// Returns what changed, or `None` if nothing visible did.
    pub fn apply(&self, verified: &VerifiedOp) -> Result<Option<Change>> {
        let author = verified.author.as_bytes().as_slice();
        let lamport = verified.body.lamport;
        let now = verified.body.timestamp_ms;
        if let Op::ChannelCreate { id, .. } | Op::Message { id, .. } | Op::PageCreate { id, .. } =
            &verified.body.op
            && !is_owned_by(id, &verified.author)
        {
            bail!("op creates an id that belongs to someone else");
        }
        self.observe_lamport(lamport)?;
        Ok(Some(match &verified.body.op {
            Op::Profile { name, is_bot } => {
                self.db.execute(
                    "INSERT INTO profiles (member, name, is_bot, lamport) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (member) DO UPDATE SET name = ?2, is_bot = ?3, lamport = ?4 WHERE lamport < ?4",
                    params![author, name, is_bot, lamport],
                )?;
                Change::Members
            }
            Op::ChannelCreate { id, name } => {
                self.db.execute(
                    &format!(
                        "INSERT INTO channels (id, name, created_ms, lamport, author)
                         VALUES (:id, :name, :now, :lamport, :author)
                         ON CONFLICT (id) DO UPDATE SET name = :name, created_ms = :now,
                             lamport = :lamport, author = :author WHERE {EARLIER_WINS}"
                    ),
                    named_params! {":id": id, ":name": name, ":now": now, ":lamport": lamport, ":author": author},
                )?;
                Change::Channels
            }
            Op::Message {
                id,
                channel,
                text,
                reply_to,
            } => {
                let changed = self.db.execute(
                    &format!(
                        "INSERT INTO messages (id, channel, author, text, reply_to, lamport, created_ms)
                         VALUES (:id, :channel, :author, :text, :reply_to, :lamport, :now)
                         ON CONFLICT (id) DO UPDATE SET channel = :channel, author = :author, text = :text,
                             reply_to = :reply_to, lamport = :lamport, created_ms = :now WHERE {EARLIER_WINS}"
                    ),
                    named_params! {
                        ":id": id, ":channel": channel, ":author": author, ":text": text,
                        ":reply_to": reply_to, ":lamport": lamport, ":now": now,
                    },
                )?;
                if changed == 1 {
                    self.index_for_search("message", id, "", text)?;
                }
                Change::Messages {
                    channel: channel.clone(),
                }
            }
            Op::Reaction {
                message,
                emoji,
                active,
            } => {
                self.db.execute(
                    "INSERT INTO reactions (message, author, emoji, active, lamport) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (message, author, emoji) DO UPDATE SET active = ?4, lamport = ?5 WHERE lamport < ?5",
                    params![message, author, emoji, active, lamport],
                )?;
                let channel: Option<String> = self
                    .db
                    .query_row("SELECT channel FROM messages WHERE id = ?", [message], |row| {
                        row.get(0)
                    })
                    .optional()?;
                match channel {
                    Some(channel) => Change::Messages { channel },
                    // The message hasn't arrived yet; its arrival refreshes the view.
                    None => return Ok(None),
                }
            }
            Op::PageCreate { id, parent, title } => {
                self.db.execute(
                    "INSERT INTO pages (id, parent, created_ms, create_lamport, create_author)
                     VALUES (:id, :parent, :now, :lamport, :author)
                     ON CONFLICT (id) DO UPDATE SET parent = :parent, created_ms = :now,
                         create_lamport = :lamport, create_author = :author
                     WHERE create_lamport IS NULL OR :lamport < create_lamport
                         OR (:lamport = create_lamport AND :author < create_author)",
                    named_params! {":id": id, ":parent": parent, ":now": now, ":lamport": lamport, ":author": author},
                )?;
                self.set_title(id, title, lamport, author, now)?;
                Change::Pages
            }
            Op::PageRename { id, title } => {
                self.db
                    .execute("INSERT OR IGNORE INTO pages (id) VALUES (?)", [id])?;
                self.set_title(id, title, lamport, author, now)?;
                Change::Pages
            }
            Op::DocUpdate { page, update } => {
                self.db
                    .execute("INSERT OR IGNORE INTO pages (id) VALUES (?)", [page])?;
                let state = self.page_state(page)?;
                let merged = pages::merge(state.as_deref(), update)?;
                self.db.execute(
                    "UPDATE pages SET doc = ?, updated_ms = MAX(updated_ms, ?) WHERE id = ?",
                    params![merged, now, page],
                )?;
                self.reindex_page(page)?;
                Change::Doc {
                    page: page.clone(),
                    update: update.clone(),
                }
            }
            Op::StorageSettings { bucket } => {
                if !config::update_bucket(self, bucket, lamport)? {
                    return Ok(None);
                }
                Change::Storage
            }
        }))
    }

    fn set_title(&self, id: &str, title: &str, lamport: u64, author: &[u8], now: i64) -> Result<()> {
        self.db.execute(
            "UPDATE pages SET title = ?2, title_lamport = ?3, title_author = ?4, updated_ms = MAX(updated_ms, ?5)
             WHERE id = ?1 AND (title_lamport < ?3 OR (title_lamport = ?3 AND title_author < ?4))",
            params![id, title, lamport, author, now],
        )?;
        self.reindex_page(id)
    }

    fn reindex_page(&self, id: &str) -> Result<()> {
        let (title, state): (String, Option<Vec<u8>>) =
            self.db
                .query_row("SELECT title, doc FROM pages WHERE id = ?", [id], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
        self.index_for_search("page", id, &title, &pages::read(state.as_deref())?)
    }

    fn index_for_search(&self, kind: &str, id: &str, title: &str, body: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM search WHERE kind = ? AND ref = ?", params![kind, id])?;
        self.db.execute(
            "INSERT INTO search (kind, ref, title, body) VALUES (?, ?, ?, ?)",
            params![kind, id, title, body],
        )?;
        Ok(())
    }
}
