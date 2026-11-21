//! Private channel state: each channel's numbered log of owner ops, who owns
//! it from which position, its key generations, the keys we hold, and DM
//! members. Rows record the op that made them (`origin`).

use anyhow::{Context, Result};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::private::{
    crypto::{ChannelKey, Commitment, LabeledKey, commitment},
    ids::PrivateKind,
};

/// Private channel state, all re-derived from the log by a rebuild.
pub(super) const PRIVATE_STATE_TABLES: &[&str] = &[
    "private_channels",
    "private_owners",
    "private_positions",
    "private_generations",
    "private_keys",
    "private_adds",
    "private_closures",
    "dm_members",
    "sealed_pending",
    "private_applied",
];

/// The op a row comes from.
#[derive(Clone, Copy)]
pub struct Origin<'a> {
    pub author: &'a PublicKey,
    pub seq: u64,
}

#[derive(Clone)]
pub struct StoredKey {
    pub channel: String,
    pub generation: u64,
    pub key: ChannelKey,
}

impl StoredKey {
    pub fn labeled(&self) -> LabeledKey<'_> {
        LabeledKey {
            key: &self.key,
            channel: &self.channel,
            generation: self.generation,
        }
    }
}

fn kind_name(kind: PrivateKind) -> &'static str {
    match kind {
        PrivateKind::Channel => "channel",
        PrivateKind::Dm => "dm",
    }
}

pub(super) fn read_key(bytes: Vec<u8>) -> rusqlite::Result<PublicKey> {
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

    /// Private channels and DMs we hold state for.
    pub fn private_channel_ids(&self) -> Result<Vec<String>> {
        let mut statement = self.db.prepare("SELECT id FROM private_channels ORDER BY id")?;
        let ids = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// The position the channel's next owner op must take (its creation is 1).
    pub fn next_position(&self, channel: &str) -> Result<u64> {
        let last: Option<u64> = self.db.query_row(
            "SELECT MAX(position) FROM private_positions WHERE channel = ?",
            [channel],
            |row| row.get(0),
        )?;
        Ok(last.unwrap_or(0) + 1)
    }

    pub(crate) fn record_position(&self, channel: &str, position: u64, origin: Origin<'_>) -> Result<()> {
        self.db.execute(
            "INSERT INTO private_positions (channel, position, origin_author, origin_seq) VALUES (?, ?, ?, ?)",
            params![channel, position, origin.author.as_bytes(), origin.seq],
        )?;
        Ok(())
    }

    /// Makes `owner` the one who writes positions from `from_position` on.
    pub(crate) fn set_owner_from(
        &self,
        channel: &str,
        from_position: u64,
        owner: &PublicKey,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO private_owners (channel, from_position, owner, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?)",
            params![
                channel,
                from_position,
                owner.as_bytes(),
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    /// Who may write `position`, as far as the positions before it say.
    pub fn owner_at(&self, channel: &str, position: u64) -> Result<Option<PublicKey>> {
        let owner: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT owner FROM private_owners WHERE channel = ? AND from_position <= ?
                 ORDER BY from_position DESC LIMIT 1",
                params![channel, position],
                |row| row.get(0),
            )
            .optional()?;
        Ok(owner.map(read_key).transpose()?)
    }

    /// Who owns the channel now: whoever writes its next position.
    pub fn current_owner(&self, channel: &str) -> Result<Option<PublicKey>> {
        self.owner_at(channel, self.next_position(channel)?)
    }

    pub(crate) fn record_generation(
        &self,
        channel: &str,
        generation: u64,
        commitment: &Commitment,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO private_generations (channel, generation, commitment, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?)",
            params![
                channel,
                generation,
                commitment.as_slice(),
                origin.author.as_bytes(),
                origin.seq
            ],
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

    /// Drops keys held for `generation` that aren't the one its owner
    /// announced; nothing could ever use them.
    pub(crate) fn drop_unannounced_keys(
        &self,
        channel: &str,
        generation: u64,
        announced: &Commitment,
    ) -> Result<()> {
        for key in self.channel_keys(Some(channel))? {
            if key.generation == generation && commitment(&key.key) != *announced {
                self.db.execute(
                    "DELETE FROM private_keys WHERE channel = ? AND generation = ? AND key = ?",
                    params![channel, generation, key.key.as_slice()],
                )?;
            }
        }
        Ok(())
    }

    /// The position of the owner op that announced key `generation`.
    pub fn generation_position(&self, channel: &str, generation: u64) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row(
                "SELECT p.position FROM private_generations g JOIN private_positions p
                     ON p.channel = g.channel AND p.origin_author = g.origin_author AND p.origin_seq = g.origin_seq
                 WHERE g.channel = ? AND g.generation = ?",
                params![channel, generation],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Whether we hold the key of every generation announced before
    /// `position`. Owner ops only count under announced keys, so then we can
    /// open anything that could take `position`.
    pub fn holds_keys_announced_before(&self, channel: &str, position: u64) -> Result<bool> {
        Ok(!self.exists(
            "SELECT 1 FROM private_generations g JOIN private_positions p
                 ON p.channel = g.channel AND p.origin_author = g.origin_author AND p.origin_seq = g.origin_seq
             WHERE g.channel = ?1 AND p.position < ?2
               AND NOT EXISTS (SELECT 1 FROM private_keys k WHERE k.channel = g.channel AND k.generation = g.generation)",
            params![channel, position],
        )?)
    }

    /// The op that took `position` in the channel's owner log.
    pub fn position_origin(&self, channel: &str, position: u64) -> Result<Option<(PublicKey, u64)>> {
        let origin: Option<(Vec<u8>, u64)> = self
            .db
            .query_row(
                "SELECT origin_author, origin_seq FROM private_positions WHERE channel = ? AND position = ?",
                params![channel, position],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        origin
            .map(|(author, seq)| Ok((read_key(author)?, seq)))
            .transpose()
    }

    /// Every key we hold for `channel` (or for all channels), vouched for or
    /// not, oldest generation first.
    pub fn channel_keys(&self, channel: Option<&str>) -> Result<Vec<StoredKey>> {
        let mut statement = self.db.prepare(
            "SELECT DISTINCT channel, generation, key FROM private_keys
             WHERE ?1 IS NULL OR channel = ?1 ORDER BY channel, generation, key",
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

    /// The first DM key we got in a grant signed by `granter`.
    pub(crate) fn dm_key_granted_by(&self, channel: &str, granter: &PublicKey) -> Result<Option<StoredKey>> {
        let key: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT key FROM private_keys WHERE channel = ? AND generation = 0 AND origin_author = ?
                 ORDER BY origin_seq LIMIT 1",
                params![channel, granter.as_bytes()],
                |row| row.get(0),
            )
            .optional()?;
        key.map(|key| {
            Ok(StoredKey {
                channel: channel.to_owned(),
                generation: 0,
                key: key
                    .try_into()
                    .ok()
                    .context("a stored DM key of the wrong length")?,
            })
        })
        .transpose()
    }

    /// Whether we hold the key the owner announced for `channel`'s newest
    /// generation (not just any key granted under that number).
    pub fn holds_newest_key(&self, channel: &str) -> Result<bool> {
        let newest = self.latest_generation(channel)?;
        let Some(announced) = self.commitment_of(channel, newest)? else {
            return Ok(false);
        };
        Ok(self
            .channel_keys(Some(channel))?
            .iter()
            .any(|key| key.generation == newest && commitment(&key.key) == announced))
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

    pub(super) fn clear_private_state(&self) -> Result<()> {
        for table in PRIVATE_STATE_TABLES {
            self.db.execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }
}
