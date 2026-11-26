//! Device certificates: a person's existing device vouches for a new one, and
//! the new device signs that it agrees to belong to that person. Both
//! signatures name the workspace, so a certificate means nothing elsewhere.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

const ACCEPTANCE_LABEL: &str = "shyspace device acceptance v1";
const CERTIFICATE_LABEL: &str = "shyspace device certificate v1";
const CERTIFICATE_HASH_CONTEXT: &str = "shyspace device certificate hash v1";
const MAP_DIGEST_CONTEXT: &str = "shyspace device map v1";

/// The rules that decide which certificates count. A commit made under
/// higher rules asks for an update, as every device must count alike.
pub const DEVICE_RULES: u32 = 1;

/// Says that `device` belongs to `person`, vouched for by `voucher` (one of
/// the person's devices at `epoch`) and accepted by `device` itself.
///
/// Many byte strings decode to the same certificate (postcard accepts
/// overlong integers and trailing bytes), so bytes received from others are
/// never hashed or compared: use [`DeviceCertificate::to_bytes`], which is
/// built from the fields, and decode with [`DeviceCertificate::from_bytes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCertificate {
    pub person: PublicKey,
    pub device: PublicKey,
    pub voucher: PublicKey,
    /// The voucher's epoch when it signed.
    pub epoch: u64,
    pub voucher_signature: Vec<u8>,
    pub device_signature: Vec<u8>,
}

/// A certificate that counted, and the epoch its commit moved to: from then
/// on the device is its person's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub certificate: DeviceCertificate,
    pub added_epoch: u64,
}

#[cfg(test)]
thread_local! {
    /// How many certificates this thread has verified, to show which never are.
    pub(crate) static VERIFIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Where a removed device's log ends, as a commit that binds the device map
/// froze it: in that map's digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundFreeze {
    pub member: PublicKey,
    pub last_seq: u64,
    /// The epoch the freezing commit moved to.
    pub epoch: u64,
}

/// What every member hands MLS as the pre-shared key of a commit that binds
/// the device map, so only devices holding the same map share the next
/// epoch's keys. `records` is the map before the commit; `frozen` holds the
/// freezes bound so far, the commit's own included. Both are sorted here.
pub fn map_digest(workspace_id: &str, records: &[DeviceRecord], frozen: &[BoundFreeze]) -> Result<[u8; 32]> {
    let mut devices = records
        .iter()
        .map(|record| {
            let certificate = &record.certificate;
            Ok((
                *certificate.device.as_bytes(),
                *certificate.person.as_bytes(),
                record.added_epoch,
                certificate.hash()?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    devices.sort();
    let mut freezes: Vec<_> = frozen
        .iter()
        .map(|freeze| (*freeze.member.as_bytes(), freeze.last_seq, freeze.epoch))
        .collect();
    freezes.sort();
    let bytes = postcard::to_stdvec(&(DEVICE_RULES, workspace_id, devices, freezes))?;
    Ok(blake3::derive_key(MAP_DIGEST_CONTEXT, &bytes))
}

/// The new device's consent to belong to `person`, sent with its request.
pub fn accept(device_key: &SecretKey, workspace_id: &str, person: &PublicKey) -> Result<Vec<u8>> {
    let bytes = acceptance_bytes(workspace_id, person, &device_key.public())?;
    Ok(device_key.sign(&bytes).to_bytes().to_vec())
}

fn acceptance_bytes(workspace_id: &str, person: &PublicKey, device: &PublicKey) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        ACCEPTANCE_LABEL,
        workspace_id,
        person.as_bytes(),
        device.as_bytes(),
    ))?)
}

fn voucher_bytes(
    workspace_id: &str,
    person: &PublicKey,
    device: &PublicKey,
    voucher: &PublicKey,
    epoch: u64,
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        CERTIFICATE_LABEL,
        workspace_id,
        person.as_bytes(),
        device.as_bytes(),
        voucher.as_bytes(),
        epoch,
    ))?)
}

fn verify_signature(signer: &PublicKey, bytes: &[u8], signature: &[u8]) -> Result<()> {
    let signature: [u8; 64] = signature.try_into().context("malformed signature")?;
    signer.verify(bytes, &Signature::from_bytes(&signature))?;
    Ok(())
}

impl DeviceCertificate {
    /// Vouches for `device` as one of `person`'s devices. Refuses unless
    /// `device_signature` is that device's acceptance for this workspace.
    /// It doesn't check that the voucher is one of the person's devices, nor
    /// that `epoch` is current: the caller knows the members and the epoch.
    pub fn vouch(
        voucher_key: &SecretKey,
        workspace_id: &str,
        person: PublicKey,
        device: PublicKey,
        epoch: u64,
        device_signature: Vec<u8>,
    ) -> Result<Self> {
        let voucher = voucher_key.public();
        let unsigned = Self {
            person,
            device,
            voucher,
            epoch,
            voucher_signature: Vec::new(),
            device_signature,
        };
        unsigned.check_roles()?;
        verify_signature(
            &device,
            &acceptance_bytes(workspace_id, &person, &device)?,
            &unsigned.device_signature,
        )
        .context("the device hasn't accepted belonging to this person here")?;
        let voucher_signature = voucher_key
            .sign(&voucher_bytes(workspace_id, &person, &device, &voucher, epoch)?)
            .to_bytes()
            .to_vec();
        Ok(Self {
            voucher_signature,
            ..unsigned
        })
    }

    /// Checks both signatures for this workspace, and that the device is
    /// neither its voucher nor the person. Whether the voucher may vouch is
    /// for the caller, who knows the members.
    pub fn verify(&self, workspace_id: &str) -> Result<()> {
        #[cfg(test)]
        VERIFIED.with(|verified| verified.set(verified.get() + 1));
        self.check_roles()?;
        verify_signature(
            &self.voucher,
            &voucher_bytes(
                workspace_id,
                &self.person,
                &self.device,
                &self.voucher,
                self.epoch,
            )?,
            &self.voucher_signature,
        )
        .context("device certificate not signed by its voucher")?;
        verify_signature(
            &self.device,
            &acceptance_bytes(workspace_id, &self.person, &self.device)?,
            &self.device_signature,
        )
        .context("device certificate not accepted by its device")?;
        Ok(())
    }

    /// The one encoding of this certificate, built from its fields.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
    }

    /// The one hash of this certificate, whichever way it arrived.
    pub fn hash(&self) -> Result<[u8; 32]> {
        Ok(blake3::derive_key(CERTIFICATE_HASH_CONTEXT, &self.to_bytes()?))
    }

    /// Decodes a certificate, refusing any bytes but its one encoding.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (certificate, rest) =
            postcard::take_from_bytes::<Self>(bytes).context("unreadable device certificate")?;
        ensure!(rest.is_empty(), "device certificate followed by extra bytes");
        ensure!(
            certificate.to_bytes()? == bytes,
            "device certificate not in its one encoding"
        );
        Ok(certificate)
    }

    fn check_roles(&self) -> Result<()> {
        ensure!(self.device != self.voucher, "a device can't vouch for itself");
        ensure!(
            self.device != self.person,
            "a person's first device needs no certificate"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "workspace-1";

    struct Keys {
        person: SecretKey,
        voucher: SecretKey,
        device: SecretKey,
    }

    fn keys() -> Keys {
        let person = SecretKey::generate();
        Keys {
            voucher: person.clone(),
            person,
            device: SecretKey::generate(),
        }
    }

    fn certificate(keys: &Keys) -> DeviceCertificate {
        let acceptance = accept(&keys.device, WORKSPACE, &keys.person.public()).unwrap();
        DeviceCertificate::vouch(
            &keys.voucher,
            WORKSPACE,
            keys.person.public(),
            keys.device.public(),
            7,
            acceptance,
        )
        .unwrap()
    }

    #[test]
    fn a_certificate_signed_by_voucher_and_device_verifies() {
        let keys = keys();
        certificate(&keys).verify(WORKSPACE).unwrap();
    }

    #[test]
    fn a_certificate_vouched_by_a_later_device_of_the_person_verifies() {
        let keys = Keys {
            voucher: SecretKey::generate(),
            ..keys()
        };
        let certificate = certificate(&keys);
        assert_eq!(certificate.voucher, keys.voucher.public());
        certificate.verify(WORKSPACE).unwrap();
    }

    #[test]
    fn a_certificate_round_trips_through_its_encoding() {
        let certificate = certificate(&keys());
        let decoded = DeviceCertificate::from_bytes(&certificate.to_bytes().unwrap()).unwrap();
        assert_eq!(decoded, certificate);
        decoded.verify(WORKSPACE).unwrap();
    }

    #[test]
    fn only_a_certificates_one_encoding_decodes() {
        let certificate = DeviceCertificate {
            epoch: 7,
            ..certificate(&keys())
        };
        let bytes = certificate.to_bytes().unwrap();
        let with_extra_bytes = [bytes.as_slice(), &[0]].concat();
        // The epoch, 7, follows the three 32-byte keys; 0x87 0x00 also reads as 7.
        assert_eq!(bytes[96], 7);
        let with_overlong_epoch = [&bytes[..96], &[0x87, 0x00], &bytes[97..]].concat();
        assert_eq!(
            postcard::from_bytes::<DeviceCertificate>(&with_overlong_epoch).unwrap(),
            certificate,
            "postcard alone accepts it"
        );
        for other_encoding in [with_extra_bytes, with_overlong_epoch] {
            assert!(DeviceCertificate::from_bytes(&other_encoding).is_err());
        }
    }

    #[test]
    fn the_signed_bytes_and_the_encoding_stay_the_same() {
        let keys = Keys {
            person: SecretKey::from_bytes(&[1; 32]),
            voucher: SecretKey::from_bytes(&[2; 32]),
            device: SecretKey::from_bytes(&[3; 32]),
        };
        let certificate = certificate(&keys);
        let (person, device, voucher) = (keys.person.public(), keys.device.public(), keys.voucher.public());

        let acceptance = acceptance_bytes(WORKSPACE, &person, &device).unwrap();
        let expected_acceptance = [
            &[ACCEPTANCE_LABEL.len() as u8][..],
            ACCEPTANCE_LABEL.as_bytes(),
            &[WORKSPACE.len() as u8],
            WORKSPACE.as_bytes(),
            person.as_bytes(),
            device.as_bytes(),
        ]
        .concat();
        assert_eq!(acceptance, expected_acceptance);
        assert_eq!((acceptance.len(), acceptance[0]), (106, 29));

        let vouched = voucher_bytes(WORKSPACE, &person, &device, &voucher, 7).unwrap();
        let expected_vouched = [
            &[CERTIFICATE_LABEL.len() as u8][..],
            CERTIFICATE_LABEL.as_bytes(),
            &[WORKSPACE.len() as u8],
            WORKSPACE.as_bytes(),
            person.as_bytes(),
            device.as_bytes(),
            voucher.as_bytes(),
            &[7],
        ]
        .concat();
        assert_eq!(vouched, expected_vouched);
        assert_eq!((vouched.len(), vouched[0]), (140, 30));

        assert_eq!(hex::encode(certificate.to_bytes().unwrap()), KNOWN_CERTIFICATE);
        assert_eq!(certificate.to_bytes().unwrap().len(), 227);
    }

    /// Person, device and voucher keys from bytes 1, 3 and 2, epoch 7; then
    /// the two 64-byte signatures, each after its length.
    const KNOWN_CERTIFICATE: &str = concat!(
        "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c",
        "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1",
        "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394",
        "0740919840acf29983fcdf20b2d6d3390b7e0be8c755c259e8e02e9f98e73862",
        "e2fbcdab17a039ef7cd1cd1d10fc3968fc04ccdfd7239340799d312ccd280bad",
        "eb004056c7d4e7e787567cdb4fff1c95d629b41624e4ba56e456b012f4372201",
        "6a9fc642ac96199a5a29b092db39e1508ceebef95002ac639fca5afa7c3998bc",
        "07450a",
    );

    #[test]
    fn a_certificate_for_another_workspace_fails() {
        let certificate = certificate(&keys());
        assert!(certificate.verify("workspace-2").is_err());
    }

    #[test]
    fn a_certificate_with_either_signature_missing_or_swapped_fails() {
        let certificate = certificate(&keys());
        let swapped = DeviceCertificate {
            voucher_signature: certificate.device_signature.clone(),
            device_signature: certificate.voucher_signature.clone(),
            ..certificate.clone()
        };
        let without_voucher = DeviceCertificate {
            voucher_signature: Vec::new(),
            ..certificate.clone()
        };
        let without_device = DeviceCertificate {
            device_signature: Vec::new(),
            ..certificate.clone()
        };
        let both_by_voucher = DeviceCertificate {
            device_signature: certificate.voucher_signature.clone(),
            ..certificate.clone()
        };
        let mut wrong_lengths = Vec::new();
        for signature in [&certificate.voucher_signature, &certificate.device_signature] {
            for length in [63, 65] {
                let mut resized = signature.clone();
                resized.resize(length, 0);
                let mut broken = certificate.clone();
                if signature == &certificate.voucher_signature {
                    broken.voucher_signature = resized;
                } else {
                    broken.device_signature = resized;
                }
                wrong_lengths.push(broken);
            }
        }
        let broken = [swapped, without_voucher, without_device, both_by_voucher]
            .into_iter()
            .chain(wrong_lengths);
        for broken in broken {
            assert!(broken.verify(WORKSPACE).is_err(), "{broken:?}");
        }
    }

    #[test]
    fn a_certificate_whose_person_device_voucher_or_epoch_was_changed_fails() {
        let certificate = certificate(&keys());
        let stranger = SecretKey::generate().public();
        let altered = [
            DeviceCertificate {
                person: stranger,
                ..certificate.clone()
            },
            DeviceCertificate {
                device: stranger,
                ..certificate.clone()
            },
            DeviceCertificate {
                voucher: stranger,
                ..certificate.clone()
            },
            DeviceCertificate {
                epoch: certificate.epoch + 1,
                ..certificate.clone()
            },
        ];
        for broken in altered {
            assert!(broken.verify(WORKSPACE).is_err(), "{broken:?}");
        }
    }

    #[test]
    fn an_acceptance_counts_only_for_its_workspace_and_person() {
        let keys = keys();
        let for_another_workspace = accept(&keys.device, "workspace-2", &keys.person.public()).unwrap();
        let for_another_person = accept(&keys.device, WORKSPACE, &SecretKey::generate().public()).unwrap();
        let by_another_key = accept(&SecretKey::generate(), WORKSPACE, &keys.person.public()).unwrap();
        for acceptance in [for_another_workspace, for_another_person, by_another_key] {
            let vouched = DeviceCertificate::vouch(
                &keys.voucher,
                WORKSPACE,
                keys.person.public(),
                keys.device.public(),
                7,
                acceptance,
            );
            assert!(vouched.is_err());
        }
    }

    #[test]
    fn a_certificate_carrying_an_acceptance_for_another_person_fails() {
        let keys = keys();
        let other_person = SecretKey::generate().public();
        let acceptance_for_other = accept(&keys.device, WORKSPACE, &other_person).unwrap();
        let voucher_signature = keys
            .voucher
            .sign(
                &voucher_bytes(
                    WORKSPACE,
                    &keys.person.public(),
                    &keys.device.public(),
                    &keys.voucher.public(),
                    7,
                )
                .unwrap(),
            )
            .to_bytes()
            .to_vec();
        let forged = DeviceCertificate {
            person: keys.person.public(),
            device: keys.device.public(),
            voucher: keys.voucher.public(),
            epoch: 7,
            voucher_signature,
            device_signature: acceptance_for_other,
        };
        assert!(forged.verify(WORKSPACE).is_err());
    }

    #[test]
    fn the_map_digest_ignores_order_and_changes_with_every_part() {
        let record = DeviceRecord {
            certificate: certificate(&keys()),
            added_epoch: 8,
        };
        let other = DeviceRecord {
            certificate: certificate(&Keys {
                device: SecretKey::generate(),
                ..keys()
            }),
            added_epoch: 9,
        };
        let freeze = BoundFreeze {
            member: SecretKey::generate().public(),
            last_seq: 4,
            epoch: 9,
        };
        let digest = map_digest(WORKSPACE, &[record.clone(), other.clone()], &[freeze]).unwrap();
        assert_eq!(
            map_digest(WORKSPACE, &[other.clone(), record.clone()], &[freeze]).unwrap(),
            digest
        );
        let later_epoch = DeviceRecord {
            added_epoch: 9,
            ..record.clone()
        };
        let later_freeze = BoundFreeze {
            last_seq: 5,
            ..freeze
        };
        for changed in [
            map_digest("workspace-2", &[record.clone(), other.clone()], &[freeze]),
            map_digest(WORKSPACE, std::slice::from_ref(&record), &[freeze]),
            map_digest(WORKSPACE, &[later_epoch, other.clone()], &[freeze]),
            map_digest(WORKSPACE, &[record.clone(), other.clone()], &[]),
            map_digest(WORKSPACE, &[record, other], &[later_freeze]),
        ] {
            assert_ne!(changed.unwrap(), digest);
        }
    }

    #[test]
    fn a_certificate_naming_the_voucher_or_the_person_as_the_device_fails() {
        let keys = Keys {
            voucher: SecretKey::generate(),
            ..keys()
        };
        for device in [&keys.voucher, &keys.person] {
            let acceptance = accept(device, WORKSPACE, &keys.person.public()).unwrap();
            let vouched = DeviceCertificate::vouch(
                &keys.voucher,
                WORKSPACE,
                keys.person.public(),
                device.public(),
                7,
                acceptance.clone(),
            );
            assert!(vouched.is_err(), "vouching refuses it");

            // Built by hand, with valid signatures, it still fails to verify.
            let voucher_signature = keys
                .voucher
                .sign(
                    &voucher_bytes(
                        WORKSPACE,
                        &keys.person.public(),
                        &device.public(),
                        &keys.voucher.public(),
                        7,
                    )
                    .unwrap(),
                )
                .to_bytes()
                .to_vec();
            let forged = DeviceCertificate {
                person: keys.person.public(),
                device: device.public(),
                voucher: keys.voucher.public(),
                epoch: 7,
                voucher_signature,
                device_signature: acceptance,
            };
            assert!(forged.verify(WORKSPACE).is_err());
        }
    }
}
