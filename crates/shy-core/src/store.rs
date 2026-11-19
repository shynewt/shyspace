//! Local SQLite database: the op log, epoch keys, and the materialized views
//! (channels, messages, pages, search) that ops are folded into.

use std::path::Path;

use anyhow::{Result, bail};
use iroh::PublicKey;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    crypto::EpochKey,
    docs,
    ops::{EntityId, Envelope, Op, VerifiedOp, is_owned_by},
    views::{Change, Channel, Member, Message, Page, Reaction, SearchHit},
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS epoch_keys (epoch INTEGER PRIMARY KEY, key BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS epoch_members (epoch INTEGER NOT NULL, member BLOB NOT NULL, PRIMARY KEY (epoch, member));
CREATE TABLE IF NOT EXISTS deliveries (joiner BLOB PRIMARY KEY, payload BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS commits (from_epoch INTEGER PRIMARY KEY, signed BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS pending_ops (
    hash BLOB PRIMARY KEY, author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS members (member BLOB PRIMARY KEY, active INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS ops (
    author BLOB NOT NULL, seq INTEGER NOT NULL, epoch INTEGER NOT NULL,
    envelope BLOB NOT NULL, applied INTEGER NOT NULL, pushed INTEGER NOT NULL,
    PRIMARY KEY (author, seq));
CREATE TABLE IF NOT EXISTS cursors (author BLOB PRIMARY KEY, last_key TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS profiles (
    member BLOB PRIMARY KEY, name TEXT NOT NULL, is_bot INTEGER NOT NULL, lamport INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS channels (
    id TEXT PRIMARY KEY, name TEXT NOT NULL, created_ms INTEGER NOT NULL,
    lamport INTEGER NOT NULL, author BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY, channel TEXT NOT NULL, author BLOB NOT NULL, text TEXT NOT NULL,
    reply_to TEXT, lamport INTEGER NOT NULL, created_ms INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS messages_by_channel ON messages (channel, lamport, author);
CREATE TABLE IF NOT EXISTS reactions (
    message TEXT NOT NULL, author BLOB NOT NULL, emoji TEXT NOT NULL,
    active INTEGER NOT NULL, lamport INTEGER NOT NULL, PRIMARY KEY (message, author, emoji));
CREATE TABLE IF NOT EXISTS pages (
    id TEXT PRIMARY KEY, parent TEXT, title TEXT NOT NULL DEFAULT '',
    title_lamport INTEGER NOT NULL DEFAULT 0, title_author BLOB,
    created_ms INTEGER, create_lamport INTEGER, create_author BLOB, updated_ms INTEGER NOT NULL DEFAULT 0, doc BLOB);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(
    kind UNINDEXED, ref UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2');
";

/// How far ahead of our own clock an op's lamport may be. Without a bound, one
/// op with lamport u64::MAX would pin titles forever and overflow every clock.
const MAX_CLOCK_JUMP: u64 = 1 << 32;
const MAX_PENDING_PER_AUTHOR: u32 = 2_000;
const MAX_PENDING_TOTAL: u32 = 20_000;

/// A creation that conflicts with an existing id keeps the earliest one by
/// (lamport, author), so every replica picks the same winner.
const EARLIER_WINS: &str = "(:lamport < lamport OR (:lamport = lamport AND :author < author))";

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db })
    }

    pub fn meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT value FROM meta WHERE key = ?", [key], |row| row.get(0))
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn meta_u64(&self, key: &str) -> Result<u64> {
        Ok(self
            .meta(key)?
            .map(|bytes| u64::from_be_bytes(bytes.try_into().unwrap_or_default()))
            .unwrap_or(0))
    }

    /// Lamport clock: one more than anything we've seen or written.
    pub fn next_lamport(&self) -> Result<u64> {
        let Some(next) = self.meta_u64("lamport")?.checked_add(1) else {
            bail!("lamport clock exhausted");
        };
        self.set_meta("lamport", &next.to_be_bytes())?;
        Ok(next)
    }

    fn observe_lamport(&self, lamport: u64) -> Result<()> {
        if lamport > self.meta_u64("lamport")? {
            self.set_meta("lamport", &lamport.to_be_bytes())?;
        }
        Ok(())
    }

    pub fn next_seq(&self, author: &PublicKey) -> Result<u64> {
        let last: Option<u64> = self.db.query_row(
            "SELECT MAX(seq) FROM ops WHERE author = ?",
            [author.as_bytes()],
            |row| row.get(0),
        )?;
        Ok(last.unwrap_or(0) + 1)
    }

    /// Remembers an epoch's key and who was a member during it. An op is
    /// only accepted if its author was a member of the op's epoch.
    pub fn put_epoch(&self, epoch: u64, key: &EpochKey, members: &[PublicKey]) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO epoch_keys (epoch, key) VALUES (?, ?)",
            params![epoch, key.as_slice()],
        )?;
        for member in members {
            self.db.execute(
                "INSERT OR IGNORE INTO epoch_members (epoch, member) VALUES (?, ?)",
                params![epoch, member.as_bytes()],
            )?;
        }
        Ok(())
    }

    pub fn epoch_members(&self, epoch: u64) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT member FROM epoch_members WHERE epoch = ?")?;
        let rows = statement.query_map([epoch], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| {
            let bytes: [u8; 32] = bytes?
                .try_into()
                .map_err(|_| anyhow::anyhow!("corrupt member id"))?;
            Ok(PublicKey::from_bytes(&bytes)?)
        })
        .collect()
    }

    pub fn was_member_at(&self, epoch: u64, member: &PublicKey) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM epoch_members WHERE epoch = ? AND member = ?",
                params![epoch, member.as_bytes()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn epoch_key(&self, epoch: u64) -> Result<Option<EpochKey>> {
        let key: Option<Vec<u8>> = self
            .db
            .query_row("SELECT key FROM epoch_keys WHERE epoch = ?", [epoch], |row| {
                row.get(0)
            })
            .optional()?;
        key.map(|bytes| bytes.try_into().map_err(|_| anyhow::anyhow!("corrupt epoch key")))
            .transpose()
    }

    pub fn epoch_keys(&self) -> Result<Vec<(u64, EpochKey)>> {
        let mut statement = self
            .db
            .prepare("SELECT epoch, key FROM epoch_keys ORDER BY epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        rows.map(|row| {
            let (epoch, key) = row?;
            Ok((
                epoch,
                key.try_into().map_err(|_| anyhow::anyhow!("corrupt epoch key"))?,
            ))
        })
        .collect()
    }

    /// Members ever seen stay listed (so old messages keep a name), but only
    /// current MLS members are active.
    pub fn set_members(&self, current: &[PublicKey]) -> Result<()> {
        self.db.execute("UPDATE members SET active = 0", [])?;
        for member in current {
            self.db.execute(
                "INSERT INTO members (member, active) VALUES (?, 1) ON CONFLICT (member) DO UPDATE SET active = 1",
                [member.as_bytes()],
            )?;
        }
        Ok(())
    }

    pub fn was_ever_member(&self, member: &PublicKey) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM epoch_members WHERE member = ? LIMIT 1",
                [member.as_bytes()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn is_applied(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM ops WHERE author = ? AND seq = ? AND applied = 1",
                params![author.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether an op's clock is close enough to ours to accept now. Ops
    /// that jump too far ahead wait as pending until our clock catches up,
    /// so replicas don't permanently disagree about them.
    pub fn accepts_clock(&self, lamport: u64) -> Result<bool> {
        Ok(lamport <= self.meta_u64("lamport")?.saturating_add(MAX_CLOCK_JUMP))
    }

    /// Records a verified, applied envelope and clears any parked copies.
    pub fn insert_applied(&self, envelope: &Envelope, pushed: bool) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO ops (author, seq, epoch, envelope, applied, pushed) VALUES (?, ?, ?, ?, 1, ?)",
            params![
                envelope.author.as_bytes(),
                envelope.seq,
                envelope.epoch,
                postcard::to_stdvec(envelope)?,
                pushed
            ],
        )?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE author = ? AND seq = ?",
            params![envelope.author.as_bytes(), envelope.seq],
        )?;
        Ok(())
    }

    /// Parks an envelope we can't apply yet. Each distinct envelope gets its
    /// own row, so a forged copy can never replace the genuine one.
    pub fn insert_pending(&self, envelope: &Envelope) -> Result<()> {
        let bytes = postcard::to_stdvec(envelope)?;
        self.db.execute(
            "INSERT OR IGNORE INTO pending_ops (hash, author, seq, epoch, envelope) VALUES (?, ?, ?, ?, ?)",
            params![
                blake3::hash(&bytes).as_bytes().as_slice(),
                envelope.author.as_bytes(),
                envelope.seq,
                envelope.epoch,
                bytes
            ],
        )?;
        // Bound what forged envelopes can make us keep: oldest go first.
        self.db.execute(
            "DELETE FROM pending_ops WHERE author = ?1 AND rowid NOT IN
                (SELECT rowid FROM pending_ops WHERE author = ?1 ORDER BY rowid DESC LIMIT ?2)",
            params![envelope.author.as_bytes(), MAX_PENDING_PER_AUTHOR],
        )?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE rowid NOT IN (SELECT rowid FROM pending_ops ORDER BY rowid DESC LIMIT ?)",
            [MAX_PENDING_TOTAL],
        )?;
        Ok(())
    }

    pub fn delete_pending(&self, envelope: &Envelope) -> Result<()> {
        let bytes = postcard::to_stdvec(envelope)?;
        self.db.execute(
            "DELETE FROM pending_ops WHERE hash = ?",
            [blake3::hash(&bytes).as_bytes().as_slice()],
        )?;
        Ok(())
    }

    pub fn put_commit_record(&self, from_epoch: u64, signed: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO commits (from_epoch, signed) VALUES (?, ?)",
            params![from_epoch, signed],
        )?;
        Ok(())
    }

    pub fn has_commit_record(&self, from_epoch: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM commits WHERE from_epoch = ?", [from_epoch], |_| {
                Ok(())
            })
            .optional()?
            .is_some())
    }

    pub fn has_ops_from(&self, author: &PublicKey) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM ops WHERE author = ? LIMIT 1",
                [author.as_bytes()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn commit_records(&self) -> Result<Vec<(u64, Vec<u8>)>> {
        let mut statement = self
            .db
            .prepare("SELECT from_epoch, signed FROM commits ORDER BY from_epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn put_delivery(&self, joiner: &PublicKey, payload: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO deliveries (joiner, payload) VALUES (?, ?)",
            params![joiner.as_bytes(), payload],
        )?;
        Ok(())
    }

    pub fn has_delivery(&self, joiner: &PublicKey) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM deliveries WHERE joiner = ?",
                [joiner.as_bytes()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn deliveries(&self) -> Result<Vec<(PublicKey, Vec<u8>)>> {
        let mut statement = self.db.prepare("SELECT joiner, payload FROM deliveries")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        rows.map(|row| {
            let (joiner, payload) = row?;
            let joiner: [u8; 32] = joiner
                .try_into()
                .map_err(|_| anyhow::anyhow!("corrupt joiner id"))?;
            Ok((PublicKey::from_bytes(&joiner)?, payload))
        })
        .collect()
    }

    pub fn delete_delivery(&self, joiner: &PublicKey) -> Result<()> {
        self.db
            .execute("DELETE FROM deliveries WHERE joiner = ?", [joiner.as_bytes()])?;
        Ok(())
    }

    fn envelopes(&self, sql: &str, args: impl rusqlite::Params) -> Result<Vec<Envelope>> {
        let mut statement = self.db.prepare(sql)?;
        let rows = statement.query_map(args, |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| Ok(postcard::from_bytes(&bytes?)?)).collect()
    }

    /// Envelopes we could not open yet because their epoch key was missing.
    pub fn pending(&self) -> Result<Vec<Envelope>> {
        self.envelopes("SELECT envelope FROM pending_ops ORDER BY epoch, seq", [])
    }

    pub fn unpushed(&self, author: &PublicKey) -> Result<Vec<Envelope>> {
        self.envelopes(
            "SELECT envelope FROM ops WHERE author = ? AND pushed = 0 ORDER BY seq",
            [author.as_bytes()],
        )
    }

    pub fn mark_pushed(&self, author: &PublicKey, up_to_seq: u64) -> Result<()> {
        self.db.execute(
            "UPDATE ops SET pushed = 1 WHERE author = ? AND seq <= ?",
            params![author.as_bytes(), up_to_seq],
        )?;
        Ok(())
    }

    pub fn cursor(&self, author: &PublicKey) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT last_key FROM cursors WHERE author = ?",
                [author.as_bytes()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_cursor(&self, author: &PublicKey, last_key: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO cursors (author, last_key) VALUES (?, ?) ON CONFLICT (author) DO UPDATE SET last_key = excluded.last_key",
            params![author.as_bytes(), last_key],
        )?;
        Ok(())
    }

    /// Folds one verified op into the views. Applying the same op twice, or
    /// ops in any order, gives the same result.
    pub fn apply(&self, verified: &VerifiedOp) -> Result<Change> {
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
        Ok(match &verified.body.op {
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
                    rusqlite::named_params! {":id": id, ":name": name, ":now": now, ":lamport": lamport, ":author": author},
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
                    rusqlite::named_params! {
                        ":id": id, ":channel": channel, ":author": author, ":text": text,
                        ":reply_to": reply_to, ":lamport": lamport, ":now": now,
                    },
                )?;
                if changed == 1 {
                    self.db
                        .execute("DELETE FROM search WHERE kind = 'message' AND ref = ?", [id])?;
                    self.db.execute(
                        "INSERT INTO search (kind, ref, title, body) VALUES ('message', ?, '', ?)",
                        params![id, text],
                    )?;
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
                let channel = self
                    .db
                    .query_row("SELECT channel FROM messages WHERE id = ?", [message], |row| {
                        row.get(0)
                    })
                    .optional()?
                    .unwrap_or_default();
                Change::Messages { channel }
            }
            Op::PageCreate { id, parent, title } => {
                self.db.execute(
                    "INSERT INTO pages (id, parent, created_ms, create_lamport, create_author)
                     VALUES (:id, :parent, :now, :lamport, :author)
                     ON CONFLICT (id) DO UPDATE SET parent = :parent, created_ms = :now,
                         create_lamport = :lamport, create_author = :author
                     WHERE create_lamport IS NULL OR :lamport < create_lamport
                         OR (:lamport = create_lamport AND :author < create_author)",
                    rusqlite::named_params! {":id": id, ":parent": parent, ":now": now, ":lamport": lamport, ":author": author},
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
                let (merged, _) = docs::merge(state.as_deref(), update)?;
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
        })
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
        let body = docs::read(state.as_deref())?;
        self.db
            .execute("DELETE FROM search WHERE kind = 'page' AND ref = ?", [id])?;
        self.db.execute(
            "INSERT INTO search (kind, ref, title, body) VALUES ('page', ?, ?, ?)",
            params![id, title, body],
        )?;
        Ok(())
    }

    pub fn page_state(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT doc FROM pages WHERE id = ?", [id], |row| row.get(0))
            .optional()?
            .flatten())
    }

    pub fn channels(&self) -> Result<Vec<Channel>> {
        let mut statement = self
            .db
            .prepare("SELECT id, name FROM channels ORDER BY created_ms, id")?;
        let rows = statement.query_map([], |row| {
            Ok(Channel {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn messages(&self, channel: &str, me: &PublicKey, limit: u32) -> Result<Vec<Message>> {
        let mut statement = self.db.prepare(
            "SELECT * FROM (
                SELECT m.id, m.channel, m.author, COALESCE(p.name, ''), COALESCE(p.is_bot, 0), m.text, m.reply_to, m.created_ms, m.lamport
                FROM messages m LEFT JOIN profiles p ON p.member = m.author
                WHERE m.channel = ? ORDER BY m.lamport DESC, m.author DESC LIMIT ?)
             ORDER BY lamport, author",
        )?;
        let rows = statement.query_map(params![channel, limit], |row| {
            Ok(Message {
                id: row.get(0)?,
                channel: row.get(1)?,
                author: hex::encode(row.get::<_, Vec<u8>>(2)?),
                author_name: row.get(3)?,
                author_is_bot: row.get(4)?,
                text: row.get(5)?,
                reply_to: row.get(6)?,
                created_ms: row.get(7)?,
                reactions: Vec::new(),
            })
        })?;
        let mut messages: Vec<Message> = rows.collect::<Result<_, _>>()?;
        for message in &mut messages {
            message.reactions = self.reactions(&message.id, me)?;
        }
        Ok(messages)
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

    pub fn message_channel(&self, id: &str) -> Result<Option<EntityId>> {
        Ok(self
            .db
            .query_row("SELECT channel FROM messages WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn pages(&self) -> Result<Vec<Page>> {
        let mut statement = self.db.prepare(
            "SELECT id, parent, title, updated_ms FROM pages WHERE created_ms IS NOT NULL ORDER BY created_ms, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Page {
                id: row.get(0)?,
                parent: row.get(1)?,
                title: row.get(2)?,
                updated_ms: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn members(&self, admin: &PublicKey) -> Result<Vec<Member>> {
        let mut statement = self.db.prepare(
            "SELECT m.member, COALESCE(p.name, ''), COALESCE(p.is_bot, 0), m.active
             FROM members m LEFT JOIN profiles p ON p.member = m.member ORDER BY m.active DESC, p.name",
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

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
        let match_expression = query
            .split_whitespace()
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if match_expression.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.db.prepare(
            "SELECT s.kind, s.ref, COALESCE(NULLIF(s.title, ''), c.name, ''), snippet(search, 3, '\u{1}', '\u{2}', '…', 12), m.channel
             FROM search s
             LEFT JOIN messages m ON s.kind = 'message' AND m.id = s.ref
             LEFT JOIN channels c ON c.id = m.channel
             WHERE search MATCH ? ORDER BY rank LIMIT ?",
        )?;
        let rows = statement.query_map(params![match_expression, limit], |row| {
            Ok(SearchHit {
                kind: row.get(0)?,
                id: row.get(1)?,
                title: row.get(2)?,
                snippet: row.get(3)?,
                channel: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
