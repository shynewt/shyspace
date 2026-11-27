//! Device changes as every member sees them: each is judged as of the epoch
//! it was sealed in, and a void one is skipped everywhere. The rows are a
//! view, rebuilt from the log; what the admin's app has acted on is kept
//! apart, since a rebuild must never make it act again.

use anyhow::Result;
use iroh::PublicKey;
use rusqlite::params;

use super::{Applied, ApplyContext, Store, apply::shown_time, to_public_key};
use crate::{
    membership::device_change::DeviceChange,
    ops::{VerifiedOp, rule_violation},
};

/// A change that passed every rule, with the person it is about.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedChange {
    pub author: PublicKey,
    pub seq: u64,
    pub epoch: u64,
    pub person: PublicKey,
    pub change: DeviceChange,
    pub shown_ms: i64,
}

impl Store {
    pub(crate) fn apply_device_change(
        &self,
        verified: &VerifiedOp,
        change: &DeviceChange,
        context: ApplyContext<'_>,
    ) -> Result<Applied> {
        change
            .check_shape()
            .map_err(|error| rule_violation(format!("{error:#}")))?;
        let person = self.person_at(&verified.author, verified.epoch)?;
        match change {
            DeviceChange::Link { certificate, .. } => {
                if certificate.person != person || certificate.voucher != verified.author {
                    return Err(rule_violation(
                        "a link vouched for by someone other than the person's device",
                    ));
                }
                certificate
                    .verify(context.workspace_id)
                    .map_err(|error| rule_violation(format!("a link with a bad certificate: {error:#}")))?;
            }
            DeviceChange::Remove { devices } => {
                for device in devices {
                    if self.person_at(device, verified.epoch)? != person {
                        return Err(rule_violation("a removal naming someone else's device"));
                    }
                }
                let is_linked_author = person != verified.author;
                if is_linked_author && devices.contains(&person) {
                    return Err(rule_violation(
                        "a linked device asking to remove the person's first device",
                    ));
                }
                if devices.contains(&context.admin) {
                    return Err(rule_violation("a removal of the admin's own device"));
                }
            }
        }
        self.observe_lamport(verified.body.lamport)?;
        self.db.execute(
            "INSERT OR IGNORE INTO device_changes (author, seq, epoch, person, change, shown_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                verified.author.as_bytes(),
                verified.seq,
                verified.epoch,
                person.as_bytes(),
                postcard::to_stdvec(change)?,
                shown_time(verified)?,
            ],
        )?;
        Ok(Applied::default())
    }

    /// Every change that passed the rules, in each author's log order.
    pub fn device_changes(&self) -> Result<Vec<RecordedChange>> {
        let mut statement = self.db.prepare(
            "SELECT author, seq, epoch, person, change, shown_ms FROM device_changes ORDER BY author, seq",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (author, seq, epoch, person, change, shown_ms) = row?;
            Ok(RecordedChange {
                author: to_public_key(author)?,
                seq,
                epoch,
                person: to_public_key(person)?,
                change: postcard::from_bytes(&change)?,
                shown_ms,
            })
        })
        .collect()
    }
}
