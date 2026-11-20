//! Sealing keys: this device's secrets for receiving private channel keys,
//! and the public key each member announced.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};
use zeroize::Zeroizing;

use super::Store;
use crate::{ops::now_ms, private::crypto::SealingKeyPair};

impl Store {
    pub fn put_sealing_secret(&self, pair: &SealingKeyPair) -> Result<()> {
        self.db.execute(
            "INSERT INTO sealing_secrets (public, secret, created_ms) VALUES (?, ?, ?)",
            params![pair.public, pair.secret.as_slice(), now_ms()?],
        )?;
        Ok(())
    }

    /// Newest first.
    pub fn sealing_secrets(&self) -> Result<Vec<SealingKeyPair>> {
        let mut statement = self
            .db
            .prepare("SELECT public, secret FROM sealing_secrets ORDER BY created_ms DESC, rowid DESC")?;
        let pairs = statement
            .query_map([], |row| {
                Ok(SealingKeyPair {
                    public: row.get(0)?,
                    secret: Zeroizing::new(row.get(1)?),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(pairs)
    }

    pub(super) fn put_sealing_key(&self, member: &PublicKey, public: &[u8], lamport: u64) -> Result<()> {
        self.db.execute(
            "INSERT INTO sealing_keys (member, public, lamport) VALUES (?1, ?2, ?3)
             ON CONFLICT (member) DO UPDATE SET public = ?2, lamport = ?3 WHERE lamport < ?3",
            params![member.as_bytes(), public, lamport],
        )?;
        Ok(())
    }

    pub fn sealing_key_of(&self, member: &PublicKey) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row(
                "SELECT public FROM sealing_keys WHERE member = ?",
                [member.as_bytes()],
                |row| row.get(0),
            )
            .optional()?)
    }
}
