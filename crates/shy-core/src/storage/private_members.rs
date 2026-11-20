//! Who belongs to a private channel when. An add starts a member's time
//! after one of their ops, from a key generation; a removal (by the owner) or
//! a leave (by the member) ends it after one of their ops, for times that
//! started before the key generation it makes.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Origin, Store, private::read_key};

/// A member's time in a channel: their ops after `after_seq`, sealed with key
/// `from_generation` or newer.
pub struct Membership {
    pub after_seq: u64,
    pub from_generation: u64,
}

/// How a member's time ended.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    RemovedByOwner,
    Left,
}

/// Times not ended by any closure of the given kinds (`?2` is 1 to count
/// leaves too, 0 for owner removals only).
const OPEN_ADD: &str = "NOT EXISTS (SELECT 1 FROM private_closures c
    WHERE c.channel = a.channel AND c.member = a.member AND (c.is_leave = 0 OR ?2 = 1)
      AND a.from_generation < c.before_generation AND c.at_seq >= a.after_seq)";

impl Store {
    pub(crate) fn record_add(
        &self,
        channel: &str,
        member: &PublicKey,
        membership: Membership,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO private_adds (channel, member, after_seq, from_generation, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                channel,
                member.as_bytes(),
                membership.after_seq,
                membership.from_generation,
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    /// Ends `member`'s times in `channel` that started before key generation
    /// `before_generation`, after their op `at_seq`. A later add starts from
    /// a newer generation, so it isn't ended too.
    pub(crate) fn record_closure(
        &self,
        channel: &str,
        member: &PublicKey,
        at_seq: u64,
        before_generation: u64,
        ending: Ending,
        origin: Origin<'_>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT INTO private_closures
                 (channel, member, at_seq, before_generation, is_leave, origin_author, origin_seq)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                channel,
                member.as_bytes(),
                at_seq,
                before_generation,
                ending == Ending::Left,
                origin.author.as_bytes(),
                origin.seq
            ],
        )?;
        Ok(())
    }

    /// The member's most recent add, if the owner hasn't removed them since.
    /// Leaves don't count here: only owner ops decide this, so every device
    /// agrees on it at each position of the channel's owner log.
    pub fn owner_membership(&self, channel: &str, member: &PublicKey) -> Result<Option<Membership>> {
        Ok(self
            .db
            .query_row(
                &format!(
                    "SELECT a.after_seq, a.from_generation FROM private_adds a
                     WHERE a.channel = ?1 AND a.member = ?3 AND {OPEN_ADD}
                     ORDER BY a.from_generation DESC, a.after_seq DESC LIMIT 1"
                ),
                params![channel, 0, member.as_bytes()],
                |row| {
                    Ok(Membership {
                        after_seq: row.get(0)?,
                        from_generation: row.get(1)?,
                    })
                },
            )
            .optional()?)
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

    /// Members whose time hasn't ended, earliest added first. With
    /// `Ending::RemovedByOwner`, those who left but weren't removed count too.
    pub fn private_members(&self, channel: &str, counting: Ending) -> Result<Vec<PublicKey>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT a.member FROM private_adds a WHERE a.channel = ?1 AND {OPEN_ADD}
             GROUP BY a.member ORDER BY MIN(a.rowid)"
        ))?;
        let members = statement
            .query_map(params![channel, counting == Ending::Left], |row| {
                read_key(row.get(0)?)
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }
}
