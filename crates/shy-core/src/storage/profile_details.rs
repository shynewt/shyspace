//! Each member's profile details: the latest by lamport clock wins.

use anyhow::Result;
use rusqlite::{OptionalExtension, named_params};

use super::Store;
use crate::profile::{ProfileDetails, WorkingHours};

impl Store {
    /// Keeps `details` (already cleaned) for `member` unless newer ones are
    /// kept. Returns whether they replaced what was there.
    pub(super) fn put_profile_details(
        &self,
        member: &[u8],
        details: &ProfileDetails,
        lamport: u64,
    ) -> Result<bool> {
        let hours = details.working_hours;
        let changed = self.db.execute(
            "INSERT INTO profile_details (member, title, pronouns, bio, email, phone, website, location,
                 time_zone, work_days, work_start, work_end, lamport)
             VALUES (:member, :title, :pronouns, :bio, :email, :phone, :website, :location,
                 :time_zone, :work_days, :work_start, :work_end, :lamport)
             ON CONFLICT (member) DO UPDATE SET title = :title, pronouns = :pronouns, bio = :bio,
                 email = :email, phone = :phone, website = :website, location = :location,
                 time_zone = :time_zone, work_days = :work_days, work_start = :work_start,
                 work_end = :work_end, lamport = :lamport
             WHERE lamport < :lamport",
            named_params! {
                ":member": member, ":title": details.title, ":pronouns": details.pronouns,
                ":bio": details.bio, ":email": details.email, ":phone": details.phone,
                ":website": details.website, ":location": details.location,
                ":time_zone": details.time_zone,
                ":work_days": hours.map(|hours| hours.days),
                ":work_start": hours.map(|hours| hours.start_minute),
                ":work_end": hours.map(|hours| hours.end_minute),
                ":lamport": lamport,
            },
        )?;
        Ok(changed > 0)
    }

    /// `member`'s details, empty if they never shared any.
    pub fn profile_details(&self, member: &[u8]) -> Result<ProfileDetails> {
        let details = self
            .db
            .query_row(
                "SELECT title, pronouns, bio, email, phone, website, location, time_zone,
                        work_days, work_start, work_end
                 FROM profile_details WHERE member = ?",
                [member],
                |row| {
                    let hours: (Option<u8>, Option<u16>, Option<u16>) =
                        (row.get(8)?, row.get(9)?, row.get(10)?);
                    Ok(ProfileDetails {
                        title: row.get(0)?,
                        pronouns: row.get(1)?,
                        bio: row.get(2)?,
                        email: row.get(3)?,
                        phone: row.get(4)?,
                        website: row.get(5)?,
                        location: row.get(6)?,
                        time_zone: row.get(7)?,
                        working_hours: match hours {
                            (Some(days), Some(start_minute), Some(end_minute)) => Some(WorkingHours {
                                days,
                                start_minute,
                                end_minute,
                            }),
                            _ => None,
                        },
                    })
                },
            )
            .optional()?;
        Ok(details.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use iroh::{PublicKey, SecretKey};

    use crate::{
        ops::{Op, OpBody, VerifiedOp},
        profile::{ProfileDetails, WorkingHours},
        storage::{Store, TEST_DB_KEY},
    };

    fn details_op(author: PublicKey, seq: u64, lamport: u64, title: &str) -> VerifiedOp {
        VerifiedOp {
            author,
            seq,
            epoch: 0,
            body: OpBody {
                lamport,
                timestamp_ms: 0,
                op: Op::ProfileDetails {
                    details: ProfileDetails {
                        title: title.into(),
                        time_zone: "Europe/Lisbon".into(),
                        working_hours: Some(WorkingHours {
                            days: 0b001_1111,
                            start_minute: 540,
                            end_minute: 1020,
                        }),
                        ..ProfileDetails::default()
                    },
                },
            },
        }
    }

    fn titles_after(ops: &[&VerifiedOp], people: &[PublicKey]) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        for _ in 0..2 {
            for op in ops {
                store.apply(op).unwrap();
            }
        }
        people
            .iter()
            .map(|person| store.profile_details(person.as_bytes()).unwrap().title)
            .collect()
    }

    #[test]
    fn each_members_latest_details_win_in_any_order_applied_twice() {
        let [ada, bob] = [(); 2].map(|_| SecretKey::generate().public());
        let ops = [
            details_op(ada, 1, 3, "Designer"),
            details_op(ada, 2, 7, "Lead designer"),
            details_op(bob, 1, 5, "Engineer"),
            // Out of their log order, as a stale lamport from a forger would be.
            details_op(bob, 2, 4, "Stale"),
        ];
        let expected = vec!["Lead designer".to_string(), "Engineer".to_string()];
        let every_order = (0..4usize.pow(4))
            .map(|code| [0, 1, 2, 3].map(|place| code / 4usize.pow(place) % 4))
            .filter(|order| (0..4).all(|index| order.contains(&index)));
        for order in every_order {
            let ordered: Vec<_> = order.iter().map(|&index| &ops[index]).collect();
            assert_eq!(titles_after(&ordered, &[ada, bob]), expected, "order {order:?}");
        }
    }

    #[test]
    fn someone_who_never_shared_details_has_empty_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let nobody = SecretKey::generate().public();
        assert_eq!(
            store.profile_details(nobody.as_bytes()).unwrap(),
            ProfileDetails::default()
        );
    }
}
