//! Epoch keys, who belonged to each epoch, removed members' frozen logs, and
//! the admin's records of commits.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, to_public_key};
use crate::{crypto::EpochKey, membership::device::BoundFreeze};

impl Store {
    /// Remembers an epoch's key and who was a member during it. An op is only
    /// accepted if its author was a member of the op's epoch.
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

    pub fn epoch_key(&self, epoch: u64) -> Result<Option<EpochKey>> {
        let key: Option<Vec<u8>> = self
            .db
            .query_row("SELECT key FROM epoch_keys WHERE epoch = ?", [epoch], |row| {
                row.get(0)
            })
            .optional()?;
        key.map(to_epoch_key).transpose()
    }

    pub fn epoch_keys(&self) -> Result<Vec<(u64, EpochKey)>> {
        let mut statement = self
            .db
            .prepare("SELECT epoch, key FROM epoch_keys ORDER BY epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, u64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        rows.map(|row| {
            let (epoch, key) = row?;
            Ok((epoch, to_epoch_key(key)?))
        })
        .collect()
    }

    pub fn epoch_members(&self, epoch: u64) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT member FROM epoch_members WHERE epoch = ?")?;
        let rows = statement.query_map([epoch], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| to_public_key(bytes?)).collect()
    }

    pub fn was_member_at(&self, epoch: u64, member: &PublicKey) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM epoch_members WHERE epoch = ? AND member = ?",
            params![epoch, member.as_bytes()],
        )
    }

    pub fn was_ever_member(&self, member: &PublicKey) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM epoch_members WHERE member = ? LIMIT 1",
            [member.as_bytes()],
        )
    }

    /// Everyone who belonged to any epoch we know of, current or former.
    pub fn everyone_ever(&self) -> Result<Vec<PublicKey>> {
        let mut statement = self.db.prepare("SELECT DISTINCT member FROM epoch_members")?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        rows.map(|bytes| to_public_key(bytes?)).collect()
    }

    /// Former members stay listed (so their messages keep a name), but only
    /// current MLS members are active.
    pub fn set_current_members(&self, current: &[PublicKey]) -> Result<()> {
        self.atomically(|store| {
            store.db.execute(
                "INSERT OR IGNORE INTO members (member, active) SELECT DISTINCT member, 0 FROM epoch_members",
                [],
            )?;
            store.db.execute("UPDATE members SET active = 0", [])?;
            for member in current {
                store.db.execute(
                    "INSERT INTO members (member, active) VALUES (?, 1) ON CONFLICT (member) DO UPDATE SET active = 1",
                    [member.as_bytes()],
                )?;
            }
            Ok(())
        })
    }

    /// A removed member's log ends at `last_seq`; anything later is forged.
    /// `frozen_epoch` is the epoch a commit binding the device map froze it
    /// in, and goes in that map's digest; older freezes have none.
    pub(super) fn record_frozen_author(
        &self,
        member: &PublicKey,
        last_seq: u64,
        frozen_epoch: Option<u64>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO frozen_authors (member, last_seq, frozen_epoch) VALUES (?, ?, ?)",
            params![member.as_bytes(), last_seq, frozen_epoch],
        )?;
        Ok(())
    }

    /// The freezes made by commits that bind the device map.
    pub fn bound_freezes(&self) -> Result<Vec<BoundFreeze>> {
        let mut statement = self.db.prepare(
            "SELECT member, last_seq, frozen_epoch FROM frozen_authors WHERE frozen_epoch IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (member, last_seq, epoch) = row?;
            Ok(BoundFreeze {
                member: to_public_key(member)?,
                last_seq,
                epoch,
            })
        })
        .collect()
    }

    pub fn frozen_authors(&self) -> Result<Vec<(PublicKey, u64)>> {
        let mut statement = self.db.prepare("SELECT member, last_seq FROM frozen_authors")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, u64>(1)?)))?;
        rows.map(|row| {
            let (member, last_seq) = row?;
            Ok((to_public_key(member)?, last_seq))
        })
        .collect()
    }

    /// Where a removed member's log ends, if they were removed.
    pub fn frozen_last_seq(&self, member: &PublicKey) -> Result<Option<u64>> {
        Ok(self
            .db
            .query_row(
                "SELECT last_seq FROM frozen_authors WHERE member = ?",
                [member.as_bytes()],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// The last epoch `member` belonged to.
    pub fn last_member_epoch(&self, member: &PublicKey) -> Result<Option<u64>> {
        Ok(self.db.query_row(
            "SELECT MAX(epoch) FROM epoch_members WHERE member = ?",
            [member.as_bytes()],
            |row| row.get(0),
        )?)
    }

    pub fn is_beyond_frozen_log(&self, member: &PublicKey, seq: u64) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM frozen_authors WHERE member = ? AND last_seq < ?",
            params![member.as_bytes(), seq],
        )
    }

    pub fn put_commit_record(&self, from_epoch: u64, signed: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO commits (from_epoch, signed) VALUES (?, ?)",
            params![from_epoch, signed],
        )?;
        Ok(())
    }

    pub fn commit_record(&self, from_epoch: u64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row(
                "SELECT signed FROM commits WHERE from_epoch = ?",
                [from_epoch],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn has_commit_record(&self, from_epoch: u64) -> Result<bool> {
        self.exists("SELECT 1 FROM commits WHERE from_epoch = ?", [from_epoch])
    }

    pub fn commit_records(&self) -> Result<Vec<(u64, Vec<u8>)>> {
        let mut statement = self
            .db
            .prepare("SELECT from_epoch, signed FROM commits ORDER BY from_epoch")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The admin's commits moving on from `first` up to `before`, in order.
    pub fn commit_records_between(&self, first: u64, before: u64) -> Result<Vec<(u64, Vec<u8>)>> {
        let mut statement = self.db.prepare(
            "SELECT from_epoch, signed FROM commits WHERE from_epoch >= ? AND from_epoch < ? ORDER BY from_epoch",
        )?;
        let rows = statement.query_map([first, before], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn to_epoch_key(bytes: Vec<u8>) -> Result<EpochKey> {
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored epoch key has the wrong length"))
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::storage::TEST_DB_KEY;

    fn active_members(store: &Store) -> Vec<Vec<u8>> {
        let mut statement = store
            .db
            .prepare("SELECT member FROM members WHERE active = 1 ORDER BY member")
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn current_members_failing_halfway_leave_the_old_ones_active() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let [ada, bob, cyd] = [(); 3].map(|()| SecretKey::generate().public());
        store.set_current_members(&[ada, bob]).unwrap();
        let before = active_members(&store);
        store.execute_for_test(&format!(
            "CREATE TRIGGER failing BEFORE INSERT ON members WHEN NEW.member = X'{}'
             BEGIN SELECT RAISE(ABORT, 'the disk is full'); END",
            hex::encode(cyd.as_bytes())
        ));

        assert!(store.set_current_members(&[ada, cyd]).is_err());

        assert_eq!(active_members(&store), before);
    }
}
