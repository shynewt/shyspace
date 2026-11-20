//! Folds verified ops into the views. Every op merges the same way whatever
//! order ops arrive in, and applying one twice changes nothing.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, named_params, params};

use super::Store;
use crate::{
    config,
    ops::{
        MAX_ID_CHARS, MAX_INDEXED_PAGE_CHARS, MAX_MESSAGE_CHARS, MAX_PAGE_TITLE_CHARS, Op, VerifiedOp,
        clean_channel_name, clean_display_name, clean_emoji, cut_to, is_owned_by, is_valid_status,
        rule_violation,
    },
    pages,
    private::{crypto::is_valid_sealing_public_key, ids::is_private_id},
    views::Change,
};

/// A creation that conflicts with an existing id keeps the earliest one by
/// (lamport, author), so every replica picks the same winner.
const EARLIER_WINS: &str = "(:lamport < lamport OR (:lamport = lamport AND :author < author))";

/// What judging an op from someone else depends on.
#[derive(Clone, Copy)]
pub struct ApplyContext<'a> {
    pub workspace_id: &'a str,
    pub admin: PublicKey,
    /// This device's member, the one private keys are wrapped for.
    pub me: PublicKey,
}

/// What applying an op did: the views that changed, and the ops it made
/// readable that may deserve an alert. A grant or a sealed op can open several
/// waiting private ops at once.
#[derive(Debug, Default)]
pub struct Applied {
    pub changes: Vec<Change>,
    pub readable: Vec<VerifiedOp>,
    /// Ops applied earlier turned out not to count (a removed member's late
    /// ones); the views must be rebuilt from the log.
    pub needs_rebuild: bool,
}

impl Applied {
    pub(crate) fn extend(&mut self, other: Applied) {
        self.changes.extend(other.changes);
        self.readable.extend(other.readable);
        self.needs_rebuild |= other.needs_rebuild;
    }
}

impl Store {
    /// Applies an op if its author may make it in this workspace;
    /// a rule violation otherwise.
    pub fn apply_permitted(&self, verified: &VerifiedOp, context: ApplyContext<'_>) -> Result<Applied> {
        if !verified.body.op.is_permitted(&verified.author, &context.admin) {
            return Err(rule_violation("only the admin may make this change"));
        }
        // Private content only travels sealed, checked by its channel's rules.
        if verified.body.op.named_ids().into_iter().any(is_private_id) {
            return Err(rule_violation("an ordinary op names a private id"));
        }
        self.apply_any(verified, context)
    }

    /// Applies an ordinary op directly, and a private one through its
    /// channel's rules.
    pub fn apply_any(&self, verified: &VerifiedOp, context: ApplyContext<'_>) -> Result<Applied> {
        match &verified.body.op {
            Op::KeyGrant { wraps } => self.apply_key_grant(verified, wraps, context),
            Op::Sealed { payload } => self.apply_sealed(verified, payload, context),
            _ => Ok(match self.apply(verified)? {
                Some(change) => Applied {
                    changes: vec![change],
                    readable: vec![verified.clone()],
                    needs_rebuild: false,
                },
                None => Applied::default(),
            }),
        }
    }

    /// Applies an ordinary op (or the content inside a sealed one). Returns
    /// what changed, or `None` if nothing visible did.
    pub fn apply(&self, verified: &VerifiedOp) -> Result<Option<Change>> {
        let author = verified.author.as_bytes().as_slice();
        let lamport = verified.body.lamport;
        let now = verified.body.timestamp_ms;
        if let Op::ChannelCreate { id, .. } | Op::Message { id, .. } | Op::PageCreate { id, .. } =
            &verified.body.op
            && !is_owned_by(id, &verified.author)
        {
            return Err(rule_violation("op creates an id that belongs to someone else"));
        }
        // A reply to a far too long id shows as a plain message, as older
        // versions show it (its parent can't be found); anything else naming
        // one is forged and goes.
        let reply_to = match &verified.body.op {
            Op::Message { reply_to, .. } => reply_to.as_deref(),
            _ => None,
        };
        let has_huge_id = verified
            .body
            .op
            .named_ids()
            .iter()
            .any(|id| id.len() > MAX_ID_CHARS && Some(*id) != reply_to);
        if has_huge_id {
            return Err(rule_violation("op names an id far longer than any real one"));
        }
        self.observe_lamport(lamport)?;
        if self.has_no_effect(&verified.body.op, verified.seq)? {
            return Ok(None);
        }
        Ok(Some(match &verified.body.op {
            Op::Profile { name, is_bot } => {
                let name = clean_display_name(name);
                self.db.execute(
                    "INSERT INTO profiles (member, name, is_bot, lamport) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (member) DO UPDATE SET name = ?2, is_bot = ?3, lamport = ?4 WHERE lamport < ?4",
                    params![author, name, is_bot, lamport],
                )?;
                Change::Members
            }
            Op::ChannelCreate { id, name } => {
                self.show_channel(id, &clean_channel_name(name), verified)?;
                Change::Channels
            }
            Op::Message {
                id,
                channel,
                text,
                reply_to,
            } => {
                self.db.execute(
                    "INSERT INTO messages (id, channel, author, text, reply_to, lamport, created_ms, seq)
                     VALUES (:id, :channel, :author, :text, :reply_to, :lamport, :now, :seq)
                     ON CONFLICT (id) DO UPDATE SET channel = :channel, author = :author, text = :text,
                         reply_to = :reply_to, lamport = :lamport, created_ms = :now, seq = :seq",
                    named_params! {
                        ":id": id, ":channel": channel, ":author": author,
                        ":text": cut_to(text, MAX_MESSAGE_CHARS),
                        ":reply_to": reply_to.as_ref().filter(|parent| parent.len() <= MAX_ID_CHARS),
                        ":lamport": lamport, ":now": now, ":seq": verified.seq,
                    },
                )?;
                self.reindex_message(id)?;
                Change::Messages {
                    channel: channel.clone(),
                }
            }
            Op::Reaction {
                message,
                emoji,
                active,
            } => {
                let emoji = clean_emoji(emoji);
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
                let merged = pages::merge(state.as_deref(), update)
                    .map_err(|error| rule_violation(format!("damaged page update: {error:#}")))?;
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
            Op::MessageEdit { id, text } => return self.apply_message_edit(id, text, lamport),
            Op::ChannelRename { id, name } => self.apply_channel_rename(id, name, lamport, author)?,
            Op::Delete { id } => return self.apply_delete(id),
            Op::Status {
                emoji,
                text,
                until_ms,
                alerts,
            } => {
                if !is_valid_status(emoji, text) {
                    return Err(rule_violation("invalid status"));
                }
                self.db.execute(
                    "INSERT INTO member_status (member, emoji, text, until_ms, alerts, lamport)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (member) DO UPDATE SET emoji = ?2, text = ?3, until_ms = ?4, alerts = ?5, lamport = ?6
                     WHERE lamport < ?6",
                    params![author, emoji, text, until_ms, *alerts as u8, lamport],
                )?;
                Change::Members
            }
            Op::KeyGrant { .. } | Op::Sealed { .. } => {
                return Err(rule_violation("a private op inside another"));
            }
            Op::SealingKey { public } => {
                if !is_valid_sealing_public_key(public) {
                    return Err(rule_violation("malformed sealing key"));
                }
                self.put_sealing_key(&verified.author, public, lamport)?;
                return Ok(None);
            }
        }))
    }

    /// Lists a channel (public, private or DM) under `name`. If two creations
    /// claim one id, the earliest wins everywhere. Returns whether the row is
    /// new.
    pub(crate) fn show_channel(&self, id: &str, name: &str, verified: &VerifiedOp) -> Result<bool> {
        let is_new = !self.has_channel_row(id)?;
        self.db.execute(
            &format!(
                "INSERT INTO channels (id, name, created_ms, lamport, author)
                 VALUES (:id, :name, :now, :lamport, :author)
                 ON CONFLICT (id) DO UPDATE SET name = :name, created_ms = :now,
                     lamport = :lamport, author = :author WHERE {EARLIER_WINS}"
            ),
            named_params! {
                ":id": id, ":name": name, ":now": verified.body.timestamp_ms,
                ":lamport": verified.body.lamport, ":author": verified.author.as_bytes().as_slice(),
            },
        )?;
        Ok(is_new)
    }

    fn has_channel_row(&self, id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM channels WHERE id = ?", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    fn set_title(&self, id: &str, title: &str, lamport: u64, author: &[u8], now: i64) -> Result<()> {
        let title = cut_to(title, MAX_PAGE_TITLE_CHARS);
        self.db.execute(
            "UPDATE pages SET title = ?2, title_lamport = ?3, title_author = ?4, updated_ms = MAX(updated_ms, ?5)
             WHERE id = ?1 AND (title_lamport < ?3 OR (title_lamport = ?3 AND title_author < ?4))",
            params![id, title, lamport, author, now],
        )?;
        self.reindex_page(id)
    }

    pub(super) fn reindex_page(&self, id: &str) -> Result<()> {
        let (title, state): (String, Option<Vec<u8>>) =
            self.db
                .query_row("SELECT title, doc FROM pages WHERE id = ?", [id], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?;
        let text = pages::read(state.as_deref())?;
        self.index_for_search("page", id, &title, cut_to(&text, MAX_INDEXED_PAGE_CHARS))
    }

    pub(super) fn index_for_search(&self, kind: &str, id: &str, title: &str, body: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM search WHERE kind = ? AND ref = ?", params![kind, id])?;
        self.db.execute(
            "INSERT INTO search (kind, ref, title, body) VALUES (?, ?, ?, ?)",
            params![kind, id, title, body],
        )?;
        Ok(())
    }
}
