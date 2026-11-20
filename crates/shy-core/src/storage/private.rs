//! Private channel state: who owns each channel, its key generations and the
//! keys we hold, who belongs when, DM members, and sealed ops still waiting.
//! Every row records the op that made it (`origin`).

use anyhow::{Context, Result, bail};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::private::{
    crypto::{ChannelKey, Commitment, KeyHint},
    ids::PrivateKind,
};

/// The op a row comes from.
#[derive(Clone, Copy)]
pub struct Origin<'a> {
    pub author: &'a PublicKey,
    pub seq: u64,
}

pub struct StoredKey {
    pub channel: String,
    pub generation: u64,
    pub key: ChannelKey,
}

pub struct Owner {
    pub member: PublicKey,
    /// The op that made them owner (for the first owner, the channel's creation).
    pub origin_seq: u64,
    pub origin_author: PublicKey,
}

/// A sealed op we couldn't apply yet. `channel` is set once one of our keys
/// matched its hint.
pub struct WaitingSealed {
    pub author: PublicKey,
    pub seq: u64,
    pub hint: KeyHint,
    pub channel: Option<String>,
}

/// Tables with an origin, in the order rows are removed.
const ORIGIN_TABLES: &[&str] = &[
    "private_channels",
    "private_owners",
    "private_generations",
    "private_keys",
    "private_adds",
    "private_closures",
];

fn kind_name(kind: PrivateKind) -> &'static str {
    match kind {
        PrivateKind::Channel => "channel",
        PrivateKind::Dm => "dm",
    }
}

fn read_key(bytes: Vec<u8>) -> rusqlite::Result<PublicKey> {
    let array: [u8; 32] = bytes.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?;
    PublicKey::from_bytes(&array).map_err(|_| rusqlite::Error::InvalidQuery)
}

impl Store {
    pub(crate) fn record_private_channel(
        &self,
        id: &str,
        kind: PrivateKind,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_channels (id, kind, origin_author, origin_seq) VALUES (?, ?, ?, ?)",
            params![id, kind_name(kind), origin.author.as_bytes(), origin.seq],
        )?;
        Ok(())
    }

    pub fn private_kind(&self, id: &str) -> Result<Option<PrivateKind>> {
        let kind: Option<String> = self
            .db
            .query_row("SELECT kind FROM private_channels WHERE id = ?", [id], |row| {
                row.get(0)
            })
            .optional()?;
        match kind.as_deref() {
            None => Ok(None),
            Some("channel") => Ok(Some(PrivateKind::Channel)),
            Some("dm") => Ok(Some(PrivateKind::Dm)),
            Some(other) => bail!("unknown private channel kind {other}"),
        }
    }

    /// Private channels and DMs we hold a key for.
    pub fn private_channel_ids(&self) -> Result<Vec<String>> {
        let mut statement = self.db.prepare("SELECT id FROM private_channels ORDER BY id")?;
        let ids = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn private_owners(&self, channel: &str) -> Result<Vec<Owner>> {
        let mut statement = self.db.prepare(
            "SELECT owner, origin_seq, origin_author FROM private_owners WHERE channel = ? ORDER BY position",
        )?;
        let owners = statement
            .query_map([channel], |row| {
                Ok(Owner {
                    member: read_key(row.get(0)?)?,
                    origin_seq: row.get(1)?,
                    origin_author: read_key(row.get(2)?)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(owners)
    }

    pub(crate) fn append_private_owner(
        &self,
        channel: &str,
        position: u32,
        owner: &PublicKey,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_owners (channel, position, owner, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?)",
            params![
                channel,
                position,
                owner.as_bytes(),
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    pub(crate) fn record_generation(
        &self,
        channel: &str,
        generation: u64,
        commitment: &Commitment,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_generations (channel, generation, commitment, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?)",
            params![channel, generation, commitment.as_slice(), origin.author.as_bytes(), origin.seq],
        )?;
        Ok(())
    }

    /// The newest key generation the channel's owners have announced, or 0.
    pub fn latest_generation(&self, channel: &str) -> Result<u64> {
        let latest: Option<u64> = self.db.query_row(
            "SELECT MAX(generation) FROM private_generations WHERE channel = ?",
            [channel],
            |row| row.get(0),
        )?;
        Ok(latest.unwrap_or(0))
    }

    pub fn commitment_of(&self, channel: &str, generation: u64) -> Result<Option<Commitment>> {
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT commitment FROM private_generations WHERE channel = ? AND generation = ?",
                params![channel, generation],
                |row| row.get(0),
            )
            .optional()?;
        bytes
            .map(|bytes| bytes.try_into().ok().context("malformed stored commitment"))
            .transpose()
    }

    /// Returns whether the key is new to us.
    pub(crate) fn put_channel_key(&self, key: &StoredKey, origin: Origin<'_>) -> Result<bool> {
        let inserted = self.db.execute(
            "INSERT OR IGNORE INTO private_keys (channel, generation, key, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?)",
            params![
                key.channel,
                key.generation,
                key.key.as_slice(),
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(inserted > 0)
    }

    /// Every key we hold for `channel` (or for all channels), checked or not.
    pub fn channel_keys(&self, channel: Option<&str>) -> Result<Vec<StoredKey>> {
        let mut statement = self.db.prepare(
            "SELECT DISTINCT channel, generation, key FROM private_keys
             WHERE ?1 IS NULL OR channel = ?1 ORDER BY channel, generation",
        )?;
        let keys = statement
            .query_map([channel], |row| {
                let key: Vec<u8> = row.get(2)?;
                Ok(StoredKey {
                    channel: row.get(0)?,
                    generation: row.get(1)?,
                    key: key.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(keys)
    }

    pub(crate) fn record_add(
        &self,
        channel: &str,
        member: &PublicKey,
        after_seq: u64,
        from_generation: u64,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_adds
                 (channel, member, after_seq, from_generation, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                channel,
                member.as_bytes(),
                after_seq,
                from_generation,
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    /// Ends `member`'s times in `channel` that started before key generation
    /// `before_generation`, after their op `at_seq`. A later add starts from a
    /// newer generation, so it isn't ended too.
    pub(crate) fn record_closure(
        &self,
        channel: &str,
        member: &PublicKey,
        at_seq: u64,
        before_generation: u64,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_closures
                 (channel, member, at_seq, before_generation, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                channel,
                member.as_bytes(),
                at_seq,
                before_generation,
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    /// Whether `member`'s op `seq`, sealed with key `generation`, falls in a
    /// time they belonged: added before it, from that generation or earlier,
    /// and not removed (or gone) between.
    pub fn is_covered(&self, channel: &str, member: &PublicKey, seq: u64, generation: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM private_adds a
                 WHERE a.channel = ?1 AND a.member = ?2 AND a.after_seq < ?3 AND a.from_generation <= ?4
                   AND NOT EXISTS (SELECT 1 FROM private_closures c
                       WHERE c.channel = a.channel AND c.member = a.member
                         AND a.from_generation < c.before_generation
                         AND c.at_seq >= a.after_seq AND c.at_seq < ?3)",
                params![channel, member.as_bytes(), seq, generation],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Members whose latest time in the channel hasn't ended, earliest added
    /// first.
    pub fn current_private_members(&self, channel: &str) -> Result<Vec<PublicKey>> {
        let mut statement = self.db.prepare(
            "SELECT a.member FROM private_adds a
             WHERE a.channel = ?1
               AND NOT EXISTS (SELECT 1 FROM private_closures c
                   WHERE c.channel = a.channel AND c.member = a.member
                     AND a.from_generation < c.before_generation AND c.at_seq >= a.after_seq)
             GROUP BY a.member ORDER BY MIN(a.rowid)",
        )?;
        let members = statement
            .query_map([channel], |row| read_key(row.get(0)?))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    /// The newest key generation one of `member`'s times in the channel was
    /// ended before, or 0.
    pub(crate) fn last_closure_generation(&self, channel: &str, member: &PublicKey) -> Result<u64> {
        let generation: Option<u64> = self.db.query_row(
            "SELECT MAX(before_generation) FROM private_closures WHERE channel = ? AND member = ?",
            params![channel, member.as_bytes()],
            |row| row.get(0),
        )?;
        Ok(generation.unwrap_or(0))
    }

    /// Members who left on their own since the newest key was made: they
    /// still hold it.
    pub(crate) fn left_since_latest_key(&self, channel: &str) -> Result<Vec<PublicKey>> {
        let mut statement = self.db.prepare(
            "SELECT DISTINCT c.member FROM private_closures c
             WHERE c.channel = ?1 AND c.origin_author = c.member
               AND c.before_generation > (SELECT COALESCE(MAX(generation), 0)
                                          FROM private_generations WHERE channel = ?1)",
        )?;
        let members = statement
            .query_map([channel], |row| read_key(row.get(0)?))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    pub(crate) fn put_dm_members(&self, channel: &str, members: &[PublicKey]) -> Result<()> {
        for member in members {
            self.db.execute(
                "INSERT OR IGNORE INTO dm_members (channel, member) VALUES (?, ?)",
                params![channel, member.as_bytes()],
            )?;
        }
        Ok(())
    }

    pub fn dm_members(&self, channel: &str) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT member FROM dm_members WHERE channel = ? ORDER BY member")?;
        let members = statement
            .query_map([channel], |row| read_key(row.get(0)?))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    pub(crate) fn record_private_applied(&self, channel: &str, author: &PublicKey, seq: u64) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO private_applied (channel, author, seq) VALUES (?, ?, ?)",
            params![channel, author.as_bytes(), seq],
        )?;
        Ok(())
    }

    pub fn is_private_applied(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM private_applied WHERE author = ? AND seq = ?",
                params![author.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Whether ops of `member`'s after `seq` in `channel` were applied here.
    pub(crate) fn has_applied_after(&self, channel: &str, member: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM private_applied WHERE channel = ? AND author = ? AND seq > ?",
                params![channel, member.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn put_waiting_sealed(&self, waiting: &WaitingSealed) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO sealed_pending (author, seq, hint, channel) VALUES (?, ?, ?, ?)",
            params![
                waiting.author.as_bytes(),
                waiting.seq,
                waiting.hint.as_slice(),
                waiting.channel
            ],
        )?;
        Ok(())
    }

    pub(crate) fn match_waiting_sealed(&self, author: &PublicKey, seq: u64, channel: &str) -> Result<()> {
        self.db.execute(
            "UPDATE sealed_pending SET channel = ? WHERE author = ? AND seq = ?",
            params![channel, author.as_bytes(), seq],
        )?;
        Ok(())
    }

    /// Waiting sealed ops, oldest first: those no key matched yet, or those
    /// already matched to a channel.
    pub(crate) fn waiting_sealed(&self, is_matched: bool) -> Result<Vec<WaitingSealed>> {
        let mut statement = self.db.prepare(
            "SELECT author, seq, hint, channel FROM sealed_pending
             WHERE (channel IS NOT NULL) = ? ORDER BY rowid",
        )?;
        let waiting = statement
            .query_map([is_matched], |row| {
                let hint: Vec<u8> = row.get(2)?;
                Ok(WaitingSealed {
                    author: read_key(row.get(0)?)?,
                    seq: row.get(1)?,
                    hint: hint.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?,
                    channel: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(waiting)
    }

    /// Whether an earlier op of `author`'s in `channel` is still waiting, so
    /// theirs apply in their own order.
    pub(crate) fn has_earlier_waiting(&self, channel: &str, author: &PublicKey, seq: u64) -> Result<bool> {
        Ok(self
            .db
            .query_row(
                "SELECT 1 FROM sealed_pending WHERE channel = ? AND author = ? AND seq < ?",
                params![channel, author.as_bytes(), seq],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn delete_waiting_sealed(&self, author: &PublicKey, seq: u64) -> Result<()> {
        self.db.execute(
            "DELETE FROM sealed_pending WHERE author = ? AND seq = ?",
            params![author.as_bytes(), seq],
        )?;
        Ok(())
    }

    /// Whether `author`'s op `seq` already made private state.
    pub(crate) fn has_origin(&self, author: &PublicKey, seq: u64) -> Result<bool> {
        for table in ORIGIN_TABLES {
            let found = self
                .db
                .query_row(
                    &format!("SELECT 1 FROM {table} WHERE origin_author = ? AND origin_seq = ? LIMIT 1"),
                    params![author.as_bytes(), seq],
                    |_| Ok(()),
                )
                .optional()?;
            if found.is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Takes out private state made by `author`'s ops after `seq`.
    pub(super) fn delete_private_rows_after(&self, author: &PublicKey, seq: u64) -> Result<()> {
        for table in ORIGIN_TABLES {
            self.db.execute(
                &format!("DELETE FROM {table} WHERE origin_author = ? AND origin_seq > ?"),
                params![author.as_bytes(), seq],
            )?;
        }
        Ok(())
    }
}
