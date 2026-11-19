//! MLS commits wrapped in the admin's identity signature. Members ignore
//! anything else in a commit slot, so junk in the bucket can't wedge the chain.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

const COMMIT_LABEL: &[u8] = b"shyspace commit v1";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SignedCommit {
    from_epoch: u64,
    commit: Vec<u8>,
    signature: Vec<u8>,
}

fn signed_message(from_epoch: u64, commit: &[u8]) -> Vec<u8> {
    [COMMIT_LABEL, &from_epoch.to_be_bytes(), commit].concat()
}

impl SignedCommit {
    pub fn sign(admin: &SecretKey, from_epoch: u64, commit: Vec<u8>) -> Result<Vec<u8>> {
        let signature = admin.sign(&signed_message(from_epoch, &commit));
        Ok(postcard::to_stdvec(&Self {
            from_epoch,
            commit,
            signature: signature.to_bytes().to_vec(),
        })?)
    }

    /// Returns the MLS commit if `bytes` is the admin's commit for `from_epoch`.
    pub fn verify(bytes: &[u8], from_epoch: u64, admin: &PublicKey) -> Result<Vec<u8>> {
        let signed: Self = postcard::from_bytes(bytes)?;
        ensure!(signed.from_epoch == from_epoch, "commit is for another epoch");
        let signature: [u8; 64] = signed
            .signature
            .as_slice()
            .try_into()
            .context("malformed signature")?;
        admin
            .verify(
                &signed_message(from_epoch, &signed.commit),
                &Signature::from_bytes(&signature),
            )
            .context("commit not signed by the admin")?;
        Ok(signed.commit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_admins_signature_for_that_epoch_verifies() {
        let admin = SecretKey::generate();
        let signed = SignedCommit::sign(&admin, 3, b"commit".to_vec()).unwrap();
        assert_eq!(
            SignedCommit::verify(&signed, 3, &admin.public()).unwrap(),
            b"commit"
        );
        assert!(SignedCommit::verify(&signed, 4, &admin.public()).is_err());
        assert!(SignedCommit::verify(&signed, 3, &SecretKey::generate().public()).is_err());
        assert!(SignedCommit::verify(b"junk", 3, &admin.public()).is_err());
    }
}
