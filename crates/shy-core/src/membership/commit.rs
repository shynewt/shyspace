//! MLS commits wrapped in the admin's identity signature. Members ignore
//! anything else in a commit slot, so junk in the bucket can't wedge the chain.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

const COMMIT_LABEL: &[u8] = b"shyspace commit v2";

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
}
