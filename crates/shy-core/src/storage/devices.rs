//! Which person each linked device belongs to, and the rules every device
//! applies to decide whether a commit's device certificate counts. A device
//! without a row is its own person.

use anyhow::{Context, Result, ensure};
use iroh::PublicKey;
use rusqlite::{OptionalExtension, params};

use super::{Store, to_public_key};
use crate::membership::{
    commit,
    device::{DeviceCertificate, DeviceRecord},
};

/// Where a device row came from: a commit this device followed, or the
/// admin's grant when this device joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSource {
    Commit,
    Grant,
}

/// At most this many of a person's devices are members at once.
pub(crate) const MAX_DEVICES_PER_PERSON: usize = 5;
/// A certificate signed in epoch `e` counts in commits leaving `e` to
/// `e + 4`, and never after: it can't be taken back, so it goes stale.
pub(crate) const CERTIFICATE_LIFETIME_EPOCHS: u64 = 4;
const JOINED_EPOCH_META: &str = "joined_epoch";

impl Store {
    /// The person `device` spoke for in `epoch`.
    pub fn person_at(&self, device: &PublicKey, epoch: u64) -> Result<PublicKey> {
        let person: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT person FROM devices WHERE device = ? AND added_epoch <= ?",
                params![device.as_bytes(), epoch],
                |row| row.get(0),
            )
            .optional()?;
        person.map_or(Ok(*device), to_public_key)
    }

    /// The person `device` belongs to, as far as this device knows now.
    pub fn person_of(&self, device: &PublicKey) -> Result<PublicKey> {
        let person: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT person FROM devices WHERE device = ?",
                [device.as_bytes()],
                |row| row.get(0),
            )
            .optional()?;
        person.map_or(Ok(*device), to_public_key)
    }

    /// The person's first device and every device linked to them, members
    /// or not.
    pub fn devices_of(&self, person: &PublicKey) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT device FROM devices WHERE person = ? ORDER BY added_epoch, device")?;
        let linked = statement.query_map([person.as_bytes()], |row| row.get::<_, Vec<u8>>(0))?;
        std::iter::once(Ok(*person))
            .chain(linked.map(|bytes| to_public_key(bytes?)))
            .collect()
    }

    /// Records that a device is its person's from `record.added_epoch` on.
    /// Putting the same record again changes nothing; a different one for
    /// the same device is refused, as a device's person never changes.
    pub fn put_device(&self, record: &DeviceRecord, source: DeviceSource, added_ms: i64) -> Result<()> {
        let certificate = &record.certificate;
        let stored: Option<(Vec<u8>, u64)> = self
            .db
            .query_row(
                "SELECT certificate, added_epoch FROM devices WHERE device = ?",
                [certificate.device.as_bytes()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stored_certificate, added_epoch)) = stored {
            ensure!(
                stored_certificate == certificate.to_bytes()? && added_epoch == record.added_epoch,
                "this device is already recorded as someone's, from another certificate or epoch"
            );
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO devices (device, person, voucher, certificate, added_epoch, from_grant, added_ms) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                certificate.device.as_bytes(),
                certificate.person.as_bytes(),
                certificate.voucher.as_bytes(),
                certificate.to_bytes()?,
                record.added_epoch,
                source == DeviceSource::Grant,
                added_ms
            ],
        )?;
        Ok(())
    }

    /// Every device `device` vouched for, directly or through a device it
    /// vouched for: what should go with it when it is stolen.
    pub fn devices_vouched_through(&self, device: &PublicKey) -> Result<Vec<PublicKey>> {
        let mut statement = self
            .db
            .prepare("SELECT device FROM devices WHERE voucher = ? ORDER BY added_epoch, device")?;
        let mut found: Vec<PublicKey> = Vec::new();
        let mut to_visit = vec![*device];
        while let Some(voucher) = to_visit.pop() {
            let vouched = statement
                .query_map([voucher.as_bytes()], |row| row.get::<_, Vec<u8>>(0))?
                .map(|row| to_public_key(row?))
                .collect::<Result<Vec<_>>>()?;
            for linked in vouched {
                if linked != *device && !found.contains(&linked) {
                    found.push(linked);
                    to_visit.push(linked);
                }
            }
        }
        Ok(found)
    }

    /// How many of `person`'s devices this device recorded linking at or
    /// after `since_ms`, removed ones included.
    pub fn links_since(&self, person: &PublicKey, since_ms: i64) -> Result<usize> {
        let count: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM devices WHERE person = ? AND added_ms >= ?",
            params![person.as_bytes(), since_ms],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count)?)
    }

    pub fn has_devices(&self) -> Result<bool> {
        self.exists("SELECT 1 FROM devices LIMIT 1", [])
    }

    /// Notes the epoch this device joined at (or created the workspace in).
    pub fn record_joined_epoch(&self, epoch: u64) -> Result<()> {
        self.set_meta(JOINED_EPOCH_META, &epoch.to_le_bytes())
    }

    /// The epoch this device joined at, if it joined on a version that notes it.
    fn joined_epoch(&self) -> Result<Option<u64>> {
        self.meta(JOINED_EPOCH_META)?
            .map(|bytes| {
                let bytes: [u8; 8] = bytes.as_slice().try_into().context("a damaged joined epoch")?;
                Ok(u64::from_le_bytes(bytes))
            })
            .transpose()
    }

    /// Counts again, oldest first, the certificates of every commit this
    /// device followed, keeping the rows it got from the grant: the map is a
    /// function of those two, so this repairs a damaged one.
    pub fn recompute_device_map(&self, workspace_id: &str, admin: &PublicKey) -> Result<()> {
        self.atomically(|store| {
            let mut added_ms = std::collections::HashMap::new();
            {
                let mut statement = store
                    .db
                    .prepare("SELECT device, added_ms FROM devices WHERE from_grant = 0")?;
                let rows =
                    statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)))?;
                for row in rows {
                    let (device, ms) = row?;
                    added_ms.insert(device, ms);
                }
            }
            store.db.execute("DELETE FROM devices WHERE from_grant = 0", [])?;
            // The grant's rows stand for every commit before the join. A
            // device that joined before this was noted kept only the commits
            // it followed.
            let joined_epoch = store.joined_epoch()?.unwrap_or_default();
            for (from_epoch, signed) in store.commit_records()? {
                if from_epoch < joined_epoch {
                    continue;
                }
                let Some(to_epoch) = from_epoch.checked_add(1) else {
                    continue;
                };
                let members_after = store.epoch_members(to_epoch)?;
                let Some(from_epoch_key) = store.epoch_key(from_epoch)? else {
                    continue;
                };
                // Only commits this device followed: it knows both epochs.
                if members_after.is_empty() || store.epoch_members(from_epoch)?.is_empty() {
                    continue;
                }
                let verified = commit::verify(&signed, from_epoch, admin)?;
                let certificates = verified.certificates(from_epoch, &from_epoch_key)?;
                for record in
                    store.count_certificates(workspace_id, from_epoch, &members_after, &certificates)?
                {
                    // A row that was missing keeps no link time; it only
                    // feeds the admin's rate limit.
                    let ms = added_ms
                        .get(record.certificate.device.as_bytes().as_slice())
                        .copied()
                        .unwrap_or_default();
                    store.put_device(&record, DeviceSource::Commit, ms)?;
                }
            }
            Ok(())
        })
    }

    /// Every device row, oldest link first.
    pub fn device_records(&self) -> Result<Vec<DeviceRecord>> {
        let mut statement = self
            .db
            .prepare("SELECT certificate, added_epoch FROM devices ORDER BY added_epoch, device")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, u64>(1)?)))?;
        rows.map(|row| {
            let (certificate, added_epoch) = row?;
            Ok(DeviceRecord {
                certificate: DeviceCertificate::from_bytes(&certificate)
                    .context("a stored device certificate is damaged")?,
                added_epoch,
            })
        })
        .collect()
    }

    /// Which of a commit's certificates count, for the commit that moves
    /// from `from_epoch` to the epoch whose members are `members_after`.
    /// One that doesn't count leaves its device its own person; the commit
    /// is followed either way.
    ///
    /// A certificate counts only in a commit that adds exactly one leaf, so
    /// at most one does. Certificates naming no added leaf are dropped
    /// before any signature is checked, and only the first naming it is
    /// looked at, so every device does the same, bounded work.
    pub fn count_certificates(
        &self,
        workspace_id: &str,
        from_epoch: u64,
        members_after: &[PublicKey],
        certificates: &[DeviceCertificate],
    ) -> Result<Vec<DeviceRecord>> {
        // Only the epoch left is checked here; the member history of earlier
        // epochs, which check 3 reads, is taken as complete.
        let members_before = self.epoch_members(from_epoch)?;
        ensure!(
            !members_before.is_empty(),
            "the members of epoch {from_epoch} aren't known, so its commit's devices can't be checked"
        );
        let is_any_key_twice = members_after
            .iter()
            .enumerate()
            .any(|(index, member)| members_after[..index].contains(member));
        if is_any_key_twice {
            return Ok(Vec::new());
        }
        let added: Vec<&PublicKey> = members_after
            .iter()
            .filter(|member| !members_before.contains(member))
            .collect();
        let [added] = added.as_slice() else {
            return Ok(Vec::new());
        };
        let Some(certificate) = certificates
            .iter()
            .find(|certificate| certificate.device == **added)
        else {
            return Ok(Vec::new());
        };
        let counts = self.certificate_counts(
            workspace_id,
            from_epoch,
            &members_before,
            members_after,
            certificate,
        )?;
        Ok(if counts {
            vec![DeviceRecord {
                certificate: certificate.clone(),
                added_epoch: from_epoch.checked_add(1).context("no epoch after the last one")?,
            }]
        } else {
            Vec::new()
        })
    }

    /// Checks 1 and 3 to 6 for the one certificate naming the added leaf.
    fn certificate_counts(
        &self,
        workspace_id: &str,
        from_epoch: u64,
        members_before: &[PublicKey],
        members_after: &[PublicKey],
        certificate: &DeviceCertificate,
    ) -> Result<bool> {
        if certificate.verify(workspace_id).is_err() {
            return Ok(false);
        }
        let DeviceCertificate {
            person,
            device,
            voucher,
            ..
        } = certificate;
        let was_member_before = self.exists(
            "SELECT 1 FROM epoch_members WHERE member = ? AND epoch <= ? LIMIT 1",
            params![device.as_bytes(), from_epoch],
        )?;
        // Rows from earlier commits only, so counting this commit again finds
        // the same answer after its own row was written.
        let has_person = self.exists(
            "SELECT 1 FROM devices WHERE device = ? AND added_epoch <= ?",
            params![device.as_bytes(), from_epoch],
        )?;
        if was_member_before || has_person {
            return Ok(false);
        }
        let can_vouch = members_before.contains(voucher)
            && members_after.contains(voucher)
            && self.person_at(voucher, from_epoch)? == *person;
        let is_fresh =
            certificate.epoch <= from_epoch && from_epoch - certificate.epoch <= CERTIFICATE_LIFETIME_EPOCHS;
        if !can_vouch || !is_fresh {
            return Ok(false);
        }
        // The map before this commit, as the digest uses.
        let mut siblings: usize = 0;
        for member in members_after.iter().filter(|member| *member != device) {
            if self.person_at(member, from_epoch)? == *person {
                siblings = siblings.saturating_add(1);
            }
        }
        Ok(siblings < MAX_DEVICES_PER_PERSON)
    }
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;
    use crate::membership::device::{VERIFIED, accept};
    use crate::storage::TEST_DB_KEY;

    const WORKSPACE: &str = "workspace-1";

    struct World {
        store: Store,
        _dir: tempfile::TempDir,
        admin: SecretKey,
        ada: SecretKey,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db"), TEST_DB_KEY).unwrap();
        let world = World {
            store,
            _dir: dir,
            admin: SecretKey::generate(),
            ada: SecretKey::generate(),
        };
        world.epoch(1, &[world.admin.public(), world.ada.public()]);
        world
    }

    impl World {
        fn epoch(&self, epoch: u64, members: &[PublicKey]) {
            self.store.put_epoch(epoch, &[epoch as u8; 32], members).unwrap();
        }

        fn members(&self, epoch: u64) -> Vec<PublicKey> {
            self.store.epoch_members(epoch).unwrap()
        }

        fn count(
            &self,
            from_epoch: u64,
            members_after: &[PublicKey],
            certificates: &[DeviceCertificate],
        ) -> usize {
            self.store
                .count_certificates(WORKSPACE, from_epoch, members_after, certificates)
                .unwrap()
                .len()
        }

        /// Links `device` to `person` as a followed commit into `epoch` would.
        fn put(&self, voucher: &SecretKey, person: &SecretKey, device: &SecretKey, epoch: u64) {
            let record = DeviceRecord {
                certificate: certificate(voucher, &person.public(), device, epoch - 1),
                added_epoch: epoch,
            };
            self.store.put_device(&record, DeviceSource::Commit, 1).unwrap();
        }
    }

    fn certificate(
        voucher: &SecretKey,
        person: &PublicKey,
        device: &SecretKey,
        epoch: u64,
    ) -> DeviceCertificate {
        let acceptance = accept(device, WORKSPACE, person).unwrap();
        DeviceCertificate::vouch(voucher, WORKSPACE, *person, device.public(), epoch, acceptance).unwrap()
    }

    fn with(members: &[PublicKey], added: &[PublicKey]) -> Vec<PublicKey> {
        members.iter().chain(added).copied().collect()
    }

    #[test]
    fn the_devices_a_device_vouched_for_are_found_through_every_link() {
        let world = world();
        let (phone, tablet, watch, other) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        world.put(&world.ada, &world.ada, &phone, 2);
        world.put(&phone, &world.ada, &tablet, 3);
        world.put(&tablet, &world.ada, &watch, 4);
        world.put(&world.ada, &world.ada, &other, 5);

        let mut found = world.store.devices_vouched_through(&phone.public()).unwrap();
        found.sort_unstable();
        let mut expected = vec![tablet.public(), watch.public()];
        expected.sort_unstable();

        assert_eq!(found, expected);
    }

    #[test]
    fn a_device_without_a_row_is_its_own_person() {
        let world = world();
        let ada = world.ada.public();
        assert_eq!(world.store.person_at(&ada, 1).unwrap(), ada);
        assert_eq!(world.store.person_of(&ada).unwrap(), ada);
        assert_eq!(world.store.devices_of(&ada).unwrap(), vec![ada]);
    }

    #[test]
    fn a_device_counts_as_its_persons_only_from_the_epoch_it_was_added() {
        let world = world();
        let phone = SecretKey::generate();
        world.put(&world.ada, &world.ada, &phone, 5);
        let ada = world.ada.public();

        assert_eq!(world.store.person_at(&phone.public(), 4).unwrap(), phone.public());
        assert_eq!(world.store.person_at(&phone.public(), 5).unwrap(), ada);
        assert_eq!(world.store.person_at(&phone.public(), 9).unwrap(), ada);
        assert_eq!(world.store.person_of(&phone.public()).unwrap(), ada);
        assert_eq!(world.store.devices_of(&ada).unwrap(), vec![ada, phone.public()]);
    }

    #[test]
    fn a_device_vouched_for_by_its_person_counts_from_the_next_epoch() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let after = with(&world.members(1), &[phone.public()]);

        let counted = world
            .store
            .count_certificates(WORKSPACE, 1, &after, std::slice::from_ref(&certificate))
            .unwrap();

        assert_eq!(
            counted,
            vec![DeviceRecord {
                certificate,
                added_epoch: 2
            }]
        );
    }

    #[test]
    fn a_linked_device_can_vouch_for_its_person() {
        let world = world();
        let laptop = SecretKey::generate();
        world.put(&world.ada, &world.ada, &laptop, 2);
        world.epoch(2, &with(&world.members(1), &[laptop.public()]));
        let phone = SecretKey::generate();
        let certificate = certificate(&laptop, &world.ada.public(), &phone, 2);

        assert_eq!(
            world.count(2, &with(&world.members(2), &[phone.public()]), &[certificate]),
            1
        );
    }

    #[test]
    fn a_forged_or_altered_certificate_does_not_count() {
        let world = world();
        let phone = SecretKey::generate();
        let genuine = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let altered = DeviceCertificate {
            epoch: 0,
            ..genuine.clone()
        };
        let signed_by_the_admin = certificate(&world.admin, &world.ada.public(), &phone, 1);
        let after = with(&world.members(1), &[phone.public()]);

        assert_eq!(world.count(1, &after, &[altered]), 0);
        assert_eq!(world.count(1, &after, &[signed_by_the_admin]), 0);
    }

    #[test]
    fn a_certificate_for_a_device_the_commit_does_not_add_does_not_count() {
        let world = world();
        let phone = SecretKey::generate();
        let stranger = SecretKey::generate().public();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);

        assert_eq!(
            world.count(1, &with(&world.members(1), &[stranger]), &[certificate]),
            0
        );
    }

    #[test]
    fn certificates_naming_no_added_leaf_are_dropped_before_any_signature_is_checked() {
        let world = world();
        let phone = SecretKey::generate();
        let decoy = certificate(&world.ada, &world.ada.public(), &SecretKey::generate(), 1);
        let mut certificates = vec![
            DeviceCertificate {
                device: SecretKey::generate().public(),
                ..decoy
            };
            1_000
        ];
        certificates.push(certificate(&world.ada, &world.ada.public(), &phone, 1));
        let before = VERIFIED.with(std::cell::Cell::get);

        let counted = world.count(1, &with(&world.members(1), &[phone.public()]), &certificates);

        assert_eq!(counted, 1);
        assert_eq!(VERIFIED.with(std::cell::Cell::get) - before, 1);
    }

    #[test]
    fn only_the_first_certificate_naming_a_device_is_looked_at() {
        let world = world();
        let phone = SecretKey::generate();
        let genuine = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let forged = DeviceCertificate {
            epoch: 0,
            ..genuine.clone()
        };
        let after = with(&world.members(1), &[phone.public()]);

        assert_eq!(world.count(1, &after, &[forged.clone(), genuine.clone()]), 0);
        assert_eq!(world.count(1, &after, &[genuine, forged]), 1);
    }

    #[test]
    fn a_key_that_was_a_member_before_cannot_become_a_device() {
        let world = world();
        let former = SecretKey::generate();
        world.epoch(0, &[world.admin.public(), former.public()]);
        let certificate = certificate(&world.ada, &world.ada.public(), &former, 1);

        assert_eq!(
            world.count(1, &with(&world.members(1), &[former.public()]), &[certificate]),
            0
        );
    }

    #[test]
    fn a_device_that_already_has_a_person_cannot_be_linked_again() {
        let world = world();
        let bob = SecretKey::generate();
        let phone = SecretKey::generate();
        world.put(&bob, &bob, &phone, 1);
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);

        assert_eq!(
            world.count(1, &with(&world.members(1), &[phone.public()]), &[certificate]),
            0
        );
    }

    #[test]
    fn a_removed_device_cannot_vouch() {
        let world = world();
        let removed = SecretKey::generate();
        world.put(&world.ada, &world.ada, &removed, 1);
        let phone = SecretKey::generate();
        let certificate = certificate(&removed, &world.ada.public(), &phone, 1);

        assert_eq!(
            world.count(1, &with(&world.members(1), &[phone.public()]), &[certificate]),
            0
        );
    }

    #[test]
    fn a_certificate_from_a_voucher_removed_in_the_same_commit_does_not_count() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);

        assert_eq!(
            world.count(1, &[world.admin.public(), phone.public()], &[certificate]),
            0
        );
    }

    #[test]
    fn a_voucher_added_by_the_same_commit_cannot_vouch() {
        let world = world();
        let laptop = SecretKey::generate();
        let phone = SecretKey::generate();
        let certificates = [
            certificate(&world.ada, &world.ada.public(), &laptop, 1),
            certificate(&laptop, &world.ada.public(), &phone, 1),
        ];
        let after = with(&world.members(1), &[laptop.public(), phone.public()]);

        assert_eq!(world.count(1, &after, &certificates), 0);
    }

    #[test]
    fn another_persons_device_cannot_vouch() {
        let world = world();
        let bob = SecretKey::generate();
        world.epoch(2, &with(&world.members(1), &[bob.public()]));
        let phone = SecretKey::generate();
        let certificate = certificate(&bob, &world.ada.public(), &phone, 2);

        assert_eq!(
            world.count(2, &with(&world.members(2), &[phone.public()]), &[certificate]),
            0
        );
    }

    #[test]
    fn a_certificate_whose_person_is_a_linked_device_or_an_unknown_key_does_not_count() {
        let world = world();
        let laptop = SecretKey::generate();
        world.put(&world.ada, &world.ada, &laptop, 2);
        world.epoch(2, &with(&world.members(1), &[laptop.public()]));
        let phone = SecretKey::generate();
        let naming_the_linked_device = certificate(&laptop, &laptop.public(), &phone, 2);
        let naming_an_unknown_key = certificate(&world.ada, &SecretKey::generate().public(), &phone, 2);
        let after = with(&world.members(2), &[phone.public()]);

        assert_eq!(world.count(2, &after, &[naming_the_linked_device]), 0);
        assert_eq!(world.count(2, &after, &[naming_an_unknown_key]), 0);
    }

    #[test]
    fn a_certificate_older_than_four_epochs_does_not_count() {
        let world = world();
        world.epoch(6, &world.members(1));
        let phone = SecretKey::generate();
        let after = with(&world.members(6), &[phone.public()]);
        let four_old = certificate(&world.ada, &world.ada.public(), &phone, 2);
        let five_old = certificate(&world.ada, &world.ada.public(), &phone, 1);

        assert_eq!(world.count(6, &after, &[five_old]), 0);
        assert_eq!(world.count(6, &after, &[four_old]), 1);
    }

    #[test]
    fn a_certificate_dated_in_the_future_does_not_count() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 2);

        assert_eq!(
            world.count(1, &with(&world.members(1), &[phone.public()]), &[certificate]),
            0
        );
    }

    #[test]
    fn a_sixth_device_does_not_count() {
        let world = world();
        // Ada's first device and three linked ones.
        let linked: Vec<SecretKey> = (0..3).map(|_| SecretKey::generate()).collect();
        for device in &linked {
            world.put(&world.ada, &world.ada, device, 1);
        }
        let members = with(
            &world.members(1),
            &linked.iter().map(SecretKey::public).collect::<Vec<_>>(),
        );
        world.epoch(2, &members);
        let fifth = SecretKey::generate();
        let sixth = SecretKey::generate();

        let fifth_certificate = certificate(&world.ada, &world.ada.public(), &fifth, 2);
        assert_eq!(
            world.count(2, &with(&members, &[fifth.public()]), &[fifth_certificate]),
            1
        );
        world.put(&world.ada, &world.ada, &fifth, 3);
        world.epoch(3, &with(&members, &[fifth.public()]));

        let sixth_certificate = certificate(&world.ada, &world.ada.public(), &sixth, 3);
        assert_eq!(
            world.count(
                3,
                &with(&world.members(3), &[sixth.public()]),
                &[sixth_certificate]
            ),
            0
        );
    }

    #[test]
    fn a_removed_device_does_not_count_toward_the_cap() {
        let world = world();
        let linked: Vec<SecretKey> = (0..4).map(|_| SecretKey::generate()).collect();
        for device in &linked {
            world.put(&world.ada, &world.ada, device, 1);
        }
        // Three of the four are members; the fourth was removed.
        let members = with(
            &world.members(1),
            &linked[..3].iter().map(SecretKey::public).collect::<Vec<_>>(),
        );
        world.epoch(2, &members);
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 2);

        assert_eq!(
            world.count(2, &with(&members, &[phone.public()]), &[certificate]),
            1
        );
    }

    #[test]
    fn a_certificate_in_a_commit_that_adds_two_leaves_does_not_count() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let after = with(
            &world.members(1),
            &[phone.public(), SecretKey::generate().public()],
        );

        assert_eq!(world.count(1, &after, &[certificate]), 0);
    }

    #[test]
    fn counting_needs_the_members_of_the_epoch_the_commit_leaves() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);

        let unknown_epoch = world
            .store
            .count_certificates(WORKSPACE, 7, &[phone.public()], &[certificate]);

        assert!(unknown_epoch.is_err());
    }

    #[test]
    fn counting_a_commit_again_after_its_row_was_written_gives_the_same_answer() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let after = with(&world.members(1), &[phone.public()]);
        let first = world
            .store
            .count_certificates(WORKSPACE, 1, &after, std::slice::from_ref(&certificate))
            .unwrap();
        world
            .store
            .put_device(&first[0], DeviceSource::Commit, 1)
            .unwrap();

        let again = world
            .store
            .count_certificates(WORKSPACE, 1, &after, &[certificate])
            .unwrap();

        assert_eq!(again, first);
    }

    #[test]
    fn a_different_record_for_a_recorded_device_is_refused() {
        let world = world();
        let phone = SecretKey::generate();
        world.put(&world.ada, &world.ada, &phone, 2);
        let as_the_admins = DeviceRecord {
            certificate: certificate(&world.admin, &world.admin.public(), &phone, 1),
            added_epoch: 2,
        };
        let at_another_epoch = DeviceRecord {
            added_epoch: 3,
            ..world.store.device_records().unwrap().remove(0)
        };

        for different in [as_the_admins, at_another_epoch] {
            assert!(
                world
                    .store
                    .put_device(&different, DeviceSource::Commit, 1)
                    .is_err()
            );
        }
        assert_eq!(
            world.store.person_of(&phone.public()).unwrap(),
            world.ada.public()
        );
    }

    #[test]
    fn a_commit_listing_a_key_twice_counts_nothing() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);
        let after = with(&world.members(1), &[phone.public(), world.admin.public()]);

        assert_eq!(world.count(1, &after, &[certificate]), 0);
    }

    #[test]
    fn a_commit_that_removes_a_member_and_adds_a_device_counts_it() {
        let world = world();
        let bob = SecretKey::generate();
        world.epoch(2, &with(&world.members(1), &[bob.public()]));
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 2);
        let after = [world.admin.public(), world.ada.public(), phone.public()];

        assert_eq!(world.count(2, &after, &[certificate]), 1);
    }

    #[test]
    fn the_cap_counts_a_persons_devices_after_their_first_one_was_removed() {
        let world = world();
        let linked: Vec<SecretKey> = (0..5).map(|_| SecretKey::generate()).collect();
        for device in &linked {
            world.put(&world.ada, &world.ada, device, 1);
        }
        // Ada's first device is gone; four linked ones are members.
        let four: Vec<PublicKey> = linked[..4].iter().map(SecretKey::public).collect();
        world.epoch(2, &with(&[world.admin.public()], &four));
        let phone = SecretKey::generate();
        let fifth = certificate(&linked[0], &world.ada.public(), &phone, 2);
        assert_eq!(
            world.count(2, &with(&world.members(2), &[phone.public()]), &[fifth]),
            1
        );

        // With five linked members, a sixth device doesn't count.
        let five: Vec<PublicKey> = linked.iter().map(SecretKey::public).collect();
        world.epoch(3, &with(&[world.admin.public()], &five));
        let sixth = certificate(&linked[0], &world.ada.public(), &phone, 3);
        assert_eq!(
            world.count(3, &with(&world.members(3), &[phone.public()]), &[sixth]),
            0
        );
    }

    #[test]
    fn putting_a_device_twice_keeps_one_row() {
        let world = world();
        let phone = SecretKey::generate();
        world.put(&world.ada, &world.ada, &phone, 2);
        world.put(&world.ada, &world.ada, &phone, 2);

        assert_eq!(world.store.device_records().unwrap().len(), 1);
    }

    #[test]
    fn the_certificate_hash_is_the_same_from_a_commit_a_row_and_a_grant() {
        let world = world();
        let phone = SecretKey::generate();
        let certificate = certificate(&world.ada, &world.ada.public(), &phone, 1);
        // A commit's list and a grant's records both carry certificates inside a larger encoding.
        let in_a_list: Vec<DeviceCertificate> =
            postcard::from_bytes(&postcard::to_stdvec(&vec![certificate.clone()]).unwrap()).unwrap();
        world.put(&world.ada, &world.ada, &phone, 2);
        let from_a_row = world.store.device_records().unwrap().remove(0).certificate;

        let hash = certificate.hash().unwrap();
        assert_eq!(in_a_list[0].hash().unwrap(), hash);
        assert_eq!(from_a_row.hash().unwrap(), hash);
    }

    #[test]
    fn a_certificate_with_an_over_long_number_encoding_hashes_like_its_plain_form() {
        let world = world();
        let certificate = DeviceCertificate {
            epoch: 1,
            ..certificate(&world.ada, &world.ada.public(), &SecretKey::generate(), 1)
        };
        let plain = postcard::to_stdvec(&vec![certificate.clone()]).unwrap();
        // The list's length, then three 32-byte keys, then the epoch.
        let epoch_at = 1 + 96;
        assert_eq!(plain[epoch_at], 1);
        let over_long = [&plain[..epoch_at], &[0x81, 0x80, 0x00], &plain[epoch_at + 1..]].concat();
        let decoded: Vec<DeviceCertificate> = postcard::from_bytes(&over_long).unwrap();

        assert_eq!(decoded[0].hash().unwrap(), certificate.hash().unwrap());
    }
}
