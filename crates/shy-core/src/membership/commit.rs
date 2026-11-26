//! MLS commits wrapped in the admin's identity signature. Members ignore
//! anything else in a commit slot, so junk in the bucket can't wedge the chain.
//!
//! Two wrappers exist. Until the workspace has a linked device, commits use
//! v2, which every version reads. From then on every commit uses v3, which
//! carries the commit's device certificates, sealed for the epoch it leaves,
//! and the number of the rules that count them.

use anyhow::{Context, Result, anyhow, bail, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use super::device::{DEVICE_RULES, DeviceCertificate};
use crate::crypto::EpochKey;

const COMMIT_LABEL: &[u8] = b"shyspace commit v2";
const COMMIT_V3_LABEL: &[u8] = b"shyspace commit v3";
const COMMIT_FORMAT: u32 = 3;
const LISTS_KEY_CONTEXT: &str = "shyspace commit lists v1";
const DEVICES_KEY_CONTEXT: &str = "shyspace commit devices v1";

/// A removed member's log ends here. Ops they sign later with an old epoch
/// key are forgeries and get rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FrozenAuthor {
    pub member: PublicKey,
    pub last_seq: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SignedPayload {
    from_epoch: u64,
    commit: Vec<u8>,
    frozen: Vec<FrozenAuthor>,
}

/// What follows the format and rules numbers in a v3 payload.
#[derive(Debug, Serialize, Deserialize)]
struct PayloadV3 {
    from_epoch: u64,
    commit: Vec<u8>,
    frozen: Vec<FrozenAuthor>,
    /// The certificate list, sealed with a key derived from the epoch the
    /// commit leaves: the bucket and people removed earlier can't read it.
    sealed_devices: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SignedCommit {
    payload: Vec<u8>,
    signature: Vec<u8>,
}

/// A commit in a newer format, or counted under newer rules, than this
/// version knows: the admin's app is newer, and this one needs an update.
#[derive(Debug)]
pub struct NewerCommit;

impl std::fmt::Display for NewerCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("the workspace admin uses a newer version of shyspace; update shyspace to keep up")
    }
}

impl std::error::Error for NewerCommit {}

/// Which wrapper a commit came in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wrapper {
    V2,
    V3 { sealed_devices: Vec<u8> },
}

/// A commit whose admin signature checked out.
#[derive(Debug)]
pub(crate) struct VerifiedCommit {
    pub commit: Vec<u8>,
    pub frozen: Vec<FrozenAuthor>,
    pub wrapper: Wrapper,
}

impl VerifiedCommit {
    /// The commit's device certificates, opened with the key of the epoch it
    /// leaves. A v2 commit carries none.
    pub fn certificates(&self, from_epoch: u64, from_epoch_key: &EpochKey) -> Result<Vec<DeviceCertificate>> {
        let Wrapper::V3 { sealed_devices } = &self.wrapper else {
            return Ok(Vec::new());
        };
        let (nonce, ciphertext) = sealed_devices
            .split_at_checked(24)
            .context("the sealed device list is too short")?;
        let nonce: [u8; 24] = nonce.try_into()?;
        let plaintext = devices_cipher(from_epoch_key)
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: &from_epoch.to_le_bytes(),
                },
            )
            .map_err(|_| anyhow!("the sealed device list doesn't open with this epoch's key"))?;
        Ok(postcard::from_bytes(&plaintext)?)
    }
}

fn devices_cipher(from_epoch_key: &EpochKey) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(&blake3::derive_key(DEVICES_KEY_CONTEXT, from_epoch_key).into())
}

/// The v2 wrapper, as every version reads it.
pub(crate) fn sign(
    admin: &SecretKey,
    from_epoch: u64,
    commit: Vec<u8>,
    frozen: Vec<FrozenAuthor>,
) -> Result<Vec<u8>> {
    let payload = postcard::to_stdvec(&SignedPayload {
        from_epoch,
        commit,
        frozen,
    })?;
    signed(admin, COMMIT_LABEL, payload)
}

/// The v3 wrapper, for every commit once the workspace has a device.
pub(crate) fn sign_v3(
    admin: &SecretKey,
    from_epoch_key: &EpochKey,
    from_epoch: u64,
    commit: Vec<u8>,
    frozen: Vec<FrozenAuthor>,
    certificates: &[DeviceCertificate],
) -> Result<Vec<u8>> {
    let nonce = XNonce::generate();
    let ciphertext = devices_cipher(from_epoch_key)
        .encrypt(
            &nonce,
            Payload {
                msg: &postcard::to_stdvec(certificates)?,
                aad: &from_epoch.to_le_bytes(),
            },
        )
        .map_err(|_| anyhow!("sealing the device list failed"))?;
    let body = postcard::to_stdvec(&PayloadV3 {
        from_epoch,
        commit,
        frozen,
        sealed_devices: [nonce.as_slice(), &ciphertext].concat(),
    })?;
    let payload = [postcard::to_stdvec(&(COMMIT_FORMAT, DEVICE_RULES))?, body].concat();
    signed(admin, COMMIT_V3_LABEL, payload)
}

/// A v3 wrapper claiming any format and rules numbers, as a newer admin's
/// app would sign it.
#[cfg(test)]
pub(crate) fn sign_in_format(admin: &SecretKey, from_epoch: u64, format: u32, rules: u32) -> Result<Vec<u8>> {
    let body = postcard::to_stdvec(&PayloadV3 {
        from_epoch,
        commit: Vec::new(),
        frozen: Vec::new(),
        sealed_devices: Vec::new(),
    })?;
    signed(
        admin,
        COMMIT_V3_LABEL,
        [postcard::to_stdvec(&(format, rules))?, body].concat(),
    )
}

fn signed(admin: &SecretKey, label: &[u8], payload: Vec<u8>) -> Result<Vec<u8>> {
    let signature = admin.sign(&[label, &payload].concat());
    Ok(postcard::to_stdvec(&SignedCommit {
        payload,
        signature: signature.to_bytes().to_vec(),
    })?)
}

/// What the admin's app puts in a commit's MLS authenticated data: a hash of
/// the lists its wrapper carries, keyed from the epoch the commit leaves. A
/// wrapper whose lists don't match is junk, so one MLS commit can't be
/// wrapped twice with different lists to split the members.
pub(crate) fn lists_hash(
    from_epoch_key: &EpochKey,
    from_epoch: u64,
    frozen: &[FrozenAuthor],
    certificates: &[DeviceCertificate],
) -> Result<[u8; 32]> {
    let key = blake3::derive_key(LISTS_KEY_CONTEXT, from_epoch_key);
    let certificate_hashes = certificates
        .iter()
        .map(DeviceCertificate::hash)
        .collect::<Result<Vec<_>>>()?;
    let lists = postcard::to_stdvec(&(from_epoch, frozen, certificate_hashes))?;
    Ok(*blake3::keyed_hash(&key, &lists).as_bytes())
}

/// Checks that `bytes` is the admin's commit for `from_epoch`, in either
/// wrapper. One in a newer format, or under newer rules, is a [`NewerCommit`].
pub(crate) fn verify(bytes: &[u8], from_epoch: u64, admin: &PublicKey) -> Result<VerifiedCommit> {
    let signed: SignedCommit = postcard::from_bytes(bytes)?;
    let signature: [u8; 64] = signed
        .signature
        .as_slice()
        .try_into()
        .context("malformed signature")?;
    let signature = Signature::from_bytes(&signature);
    let verified = if admin
        .verify(&[COMMIT_V3_LABEL, &signed.payload].concat(), &signature)
        .is_ok()
    {
        open_v3(&signed.payload)?
    } else {
        admin
            .verify(&[COMMIT_LABEL, &signed.payload].concat(), &signature)
            .context("commit not signed by the admin")?;
        let payload: SignedPayload = postcard::from_bytes(&signed.payload)?;
        (
            payload.from_epoch,
            VerifiedCommit {
                commit: payload.commit,
                frozen: payload.frozen,
                wrapper: Wrapper::V2,
            },
        )
    };
    let (signed_from_epoch, verified) = verified;
    ensure!(signed_from_epoch == from_epoch, "commit is for another epoch");
    Ok(verified)
}

fn open_v3(payload: &[u8]) -> Result<(u64, VerifiedCommit)> {
    let (format, rest) = postcard::take_from_bytes::<u32>(payload)?;
    if format > COMMIT_FORMAT {
        bail!(NewerCommit);
    }
    ensure!(format == COMMIT_FORMAT, "a v3 commit in an unknown format");
    let (rules, rest) = postcard::take_from_bytes::<u32>(rest)?;
    if rules > DEVICE_RULES {
        bail!(NewerCommit);
    }
    ensure!(rules == DEVICE_RULES, "a v3 commit under unknown rules");
    let body: PayloadV3 = postcard::from_bytes(rest)?;
    Ok((
        body.from_epoch,
        VerifiedCommit {
            commit: body.commit,
            frozen: body.frozen,
            wrapper: Wrapper::V3 {
                sealed_devices: body.sealed_devices,
            },
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_admins_signature_for_that_epoch_verifies() {
        let admin = SecretKey::generate();
        let frozen = vec![FrozenAuthor {
            member: SecretKey::generate().public(),
            last_seq: 9,
        }];
        let signed = sign(&admin, 3, b"commit".to_vec(), frozen.clone()).unwrap();
        let verified = verify(&signed, 3, &admin.public()).unwrap();
        assert_eq!(verified.commit, b"commit");
        assert_eq!(verified.frozen, frozen);
        assert!(verify(&signed, 4, &admin.public()).is_err());
        assert!(verify(&signed, 3, &SecretKey::generate().public()).is_err());
        assert!(verify(b"junk", 3, &admin.public()).is_err());
    }

    fn certificate() -> DeviceCertificate {
        let person = SecretKey::generate();
        let device = SecretKey::generate();
        let acceptance = super::super::device::accept(&device, "workspace-1", &person.public()).unwrap();
        DeviceCertificate::vouch(
            &person,
            "workspace-1",
            person.public(),
            device.public(),
            2,
            acceptance,
        )
        .unwrap()
    }

    #[test]
    fn a_v2_commit_keeps_the_layout_every_version_reads() {
        let admin = SecretKey::generate();
        let signed = sign(&admin, 3, b"commit".to_vec(), Vec::new()).unwrap();
        let wrapper: SignedCommit = postcard::from_bytes(&signed).unwrap();
        let payload: SignedPayload = postcard::from_bytes(&wrapper.payload).unwrap();
        assert_eq!(
            (payload.from_epoch, payload.commit.as_slice()),
            (3, b"commit".as_slice())
        );
        let signature = Signature::from_bytes(&wrapper.signature.try_into().unwrap());
        admin
            .public()
            .verify(
                &[b"shyspace commit v2".as_slice(), &wrapper.payload].concat(),
                &signature,
            )
            .unwrap();
        assert_eq!(verify(&signed, 3, &admin.public()).unwrap().wrapper, Wrapper::V2);
    }

    #[test]
    fn a_v3_commit_carries_its_certificates_sealed_for_the_epoch_it_leaves() {
        let admin = SecretKey::generate();
        let key = [5; 32];
        let certificates = vec![certificate()];
        let signed = sign_v3(&admin, &key, 3, b"commit".to_vec(), Vec::new(), &certificates).unwrap();

        let verified = verify(&signed, 3, &admin.public()).unwrap();

        assert_eq!(verified.certificates(3, &key).unwrap(), certificates);
        // Someone removed before epoch 3 doesn't hold its key.
        assert!(verified.certificates(3, &[6; 32]).is_err());
        assert!(
            !signed
                .windows(32)
                .any(|window| window == certificates[0].device.as_bytes())
        );
    }

    #[test]
    fn a_v3_commit_signed_by_someone_else_is_ignored() {
        let admin = SecretKey::generate();
        let key = [5; 32];
        let by_someone_else =
            sign_v3(&SecretKey::generate(), &key, 3, b"c".to_vec(), Vec::new(), &[]).unwrap();
        assert!(verify(&by_someone_else, 3, &admin.public()).is_err());
        assert!(verify(b"junk under any label", 3, &admin.public()).is_err());
    }

    #[test]
    fn a_signature_under_one_wrapper_label_never_counts_under_the_other() {
        let admin = SecretKey::generate();
        let v2 = sign(&admin, 3, b"c".to_vec(), Vec::new()).unwrap();
        let v3 = sign_v3(&admin, &[5; 32], 3, b"c".to_vec(), Vec::new(), &[]).unwrap();
        for (bytes, other_label) in [(v2, COMMIT_V3_LABEL), (v3, COMMIT_LABEL)] {
            let wrapper: SignedCommit = postcard::from_bytes(&bytes).unwrap();
            let signature = Signature::from_bytes(&wrapper.signature.try_into().unwrap());
            assert!(
                admin
                    .public()
                    .verify(&[other_label, &wrapper.payload].concat(), &signature)
                    .is_err()
            );
        }
    }

    #[test]
    fn a_commit_in_a_newer_format_or_under_newer_rules_asks_for_an_update() {
        let admin = SecretKey::generate();
        let body = postcard::to_stdvec(&PayloadV3 {
            from_epoch: 3,
            commit: Vec::new(),
            frozen: Vec::new(),
            sealed_devices: Vec::new(),
        })
        .unwrap();
        for (format, rules) in [
            (COMMIT_FORMAT + 1, DEVICE_RULES),
            (COMMIT_FORMAT, DEVICE_RULES + 1),
        ] {
            let payload = [postcard::to_stdvec(&(format, rules)).unwrap(), body.clone()].concat();
            let newer = signed(&admin, COMMIT_V3_LABEL, payload).unwrap();
            let error = verify(&newer, 3, &admin.public()).unwrap_err();
            assert!(error.downcast_ref::<NewerCommit>().is_some(), "{error:#}");
        }
    }

    #[test]
    fn the_lists_hash_changes_with_every_list_and_the_epoch_key() {
        let key = [7; 32];
        let frozen = [FrozenAuthor {
            member: SecretKey::generate().public(),
            last_seq: 9,
        }];
        let hash = lists_hash(&key, 3, &frozen, &[]).unwrap();
        assert_eq!(lists_hash(&key, 3, &frozen, &[]).unwrap(), hash);

        let one_later = [FrozenAuthor {
            last_seq: 10,
            ..frozen[0]
        }];
        for other in [
            lists_hash(&[8; 32], 3, &frozen, &[]),
            lists_hash(&key, 4, &frozen, &[]),
            lists_hash(&key, 3, &[], &[]),
            lists_hash(&key, 3, &one_later, &[]),
        ] {
            assert_ne!(other.unwrap(), hash);
        }
    }
}
