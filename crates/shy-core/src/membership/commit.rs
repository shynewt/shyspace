//! MLS commits wrapped in the admin's identity signature. Members ignore
//! anything else in a commit slot, so junk in the bucket can't wedge the chain.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use super::device::DeviceCertificate;
use crate::crypto::EpochKey;

const COMMIT_LABEL: &[u8] = b"shyspace commit v2";
const LISTS_KEY_CONTEXT: &str = "shyspace commit lists v1";

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

#[derive(Debug, Serialize, Deserialize)]
struct SignedCommit {
    payload: Vec<u8>,
    signature: Vec<u8>,
}

/// A commit whose admin signature checked out.
#[derive(Debug)]
pub(crate) struct VerifiedCommit {
    pub commit: Vec<u8>,
    pub frozen: Vec<FrozenAuthor>,
}

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
    let signature = admin.sign(&[COMMIT_LABEL, &payload].concat());
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

/// Checks that `bytes` is the admin's commit for `from_epoch`.
pub(crate) fn verify(bytes: &[u8], from_epoch: u64, admin: &PublicKey) -> Result<VerifiedCommit> {
    let signed: SignedCommit = postcard::from_bytes(bytes)?;
    let signature: [u8; 64] = signed
        .signature
        .as_slice()
        .try_into()
        .context("malformed signature")?;
    admin
        .verify(
            &[COMMIT_LABEL, &signed.payload].concat(),
            &Signature::from_bytes(&signature),
        )
        .context("commit not signed by the admin")?;
    let payload: SignedPayload = postcard::from_bytes(&signed.payload)?;
    ensure!(payload.from_epoch == from_epoch, "commit is for another epoch");
    Ok(VerifiedCommit {
        commit: payload.commit,
        frozen: payload.frozen,
    })
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
