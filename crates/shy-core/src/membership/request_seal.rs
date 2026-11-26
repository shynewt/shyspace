//! Join requests sealed to the admin (#276). A request waits in the bucket
//! until the admin looks, where anyone with the bucket keys could read the
//! name the joiner typed, so the joiner seals it with HPKE to a key the
//! invite carries. Only the admin can open it, and only for that invite.

use anyhow::{Context, Result, anyhow, ensure};
use iroh::{PublicKey, SecretKey};
use openmls_rust_crypto::RustCrypto;
use openmls_traits::{crypto::OpenMlsCrypto, types::HpkeCiphertext};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::invite::{Invite, JoinRequest};
use crate::private::crypto::{HPKE, SealingKeyPair, is_valid_sealing_public_key};

const REQUEST_KEY_CONTEXT: &str = "shyspace join request key v1";
const REQUEST_INFO_DOMAIN: &str = "shyspace join request v1";

/// What a request is when it doesn't open as one for this invite: junk,
/// one in the clear, or one sealed for another invite.
#[derive(Debug)]
pub(crate) struct NotForThisInvite;

impl std::fmt::Display for NotForThisInvite {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not a join request sealed for this invite")
    }
}

impl std::error::Error for NotForThisInvite {}

/// What a joiner posts through the request link.
#[derive(Serialize, Deserialize)]
struct SealedRequest {
    kem_output: Vec<u8>,
    ciphertext: Vec<u8>,
}

/// The invite a request is for. HPKE already binds the encapsulated key;
/// this binds the rest, so a request can't be moved to another invite.
#[derive(Clone, Copy)]
pub(crate) struct RequestSlot<'a> {
    pub workspace_id: &'a str,
    pub token: &'a str,
    pub admin: &'a PublicKey,
}

impl RequestSlot<'_> {
    fn info(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&(
            REQUEST_INFO_DOMAIN,
            self.workspace_id,
            self.token,
            self.admin.as_bytes(),
        ))?)
    }
}

/// The admin's key pair for requests through the invite `token`, derived
/// rather than stored, so a restored admin opens requests made before the
/// restore. Sound to derive from the identity key: blake3's `derive_key` is
/// a KDF with its own context string, so the result reveals nothing about
/// the identity key or any other key derived from it, and whoever holds the
/// identity key is the admin anyway.
pub(crate) fn request_key_pair(identity: &SecretKey, token: &str) -> Result<SealingKeyPair> {
    let identity_bytes = Zeroizing::new(identity.to_bytes());
    let seed = Zeroizing::new(
        *blake3::Hasher::new_derive_key(REQUEST_KEY_CONTEXT)
            .update(identity_bytes.as_slice())
            .update(token.as_bytes())
            .finalize()
            .as_bytes(),
    );
    let pair = RustCrypto::default()
        .derive_hpke_keypair(HPKE, seed.as_slice())
        .map_err(|error| anyhow!("making a join request key: {error:?}"))?;
    Ok(SealingKeyPair {
        secret: Zeroizing::new(pair.private.to_vec()),
        public: pair.public,
    })
}

pub(crate) fn seal_request(invite: &Invite, request: &JoinRequest) -> Result<Vec<u8>> {
    ensure!(
        is_valid_sealing_public_key(&invite.request_key),
        "this invite code is damaged"
    );
    let slot = RequestSlot {
        workspace_id: &invite.workspace_id,
        token: &invite.token,
        admin: &invite.admin,
    };
    let plaintext = Zeroizing::new(postcard::to_stdvec(request)?);
    let sealed = RustCrypto::default()
        .hpke_seal(HPKE, &invite.request_key, &slot.info()?, &[], &plaintext)
        .map_err(|error| anyhow!("sealing the join request: {error:?}"))?;
    Ok(postcard::to_stdvec(&SealedRequest {
        kem_output: sealed.kem_output.as_slice().to_vec(),
        ciphertext: sealed.ciphertext.as_slice().to_vec(),
    })?)
}

/// Fails with [`NotForThisInvite`] for anything but a request sealed to
/// this admin for this invite.
pub(crate) fn open_request(identity: &SecretKey, slot: RequestSlot<'_>, bytes: &[u8]) -> Result<JoinRequest> {
    let sealed: SealedRequest = postcard::from_bytes(bytes).map_err(|_| NotForThisInvite)?;
    let own = request_key_pair(identity, slot.token)?;
    let ciphertext = HpkeCiphertext {
        kem_output: sealed.kem_output.into(),
        ciphertext: sealed.ciphertext.into(),
    };
    let plaintext = RustCrypto::default()
        .hpke_open(HPKE, &ciphertext, &own.secret, &slot.info()?, &[])
        .map(Zeroizing::new)
        .map_err(|_| NotForThisInvite)?;
    JoinRequest::decode(&plaintext).context("reading an opened join request")
}

/// Whether `bytes` is a join request as versions before sealing sent it,
/// in the clear, so the admin can be told to send a new invite.
pub(crate) fn is_unsealed_request(bytes: &[u8]) -> bool {
    JoinRequest::decode(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "acme";

    fn invite_for(admin: &SecretKey, token: &str) -> Invite {
        Invite {
            workspace_id: WORKSPACE.into(),
            workspace_name: "Acme".into(),
            admin: admin.public(),
            admin_mls_key: Vec::new(),
            token: token.into(),
            expires_ms: 0,
            request_url: String::new(),
            answer_url: String::new(),
            request_key: request_key_pair(admin, token).unwrap().public,
            signs_acceptance: true,
        }
    }

    fn request() -> JoinRequest {
        JoinRequest {
            key_package: vec![1, 2, 3],
            binding: vec![4, 5],
            name: "Bob Secretname".into(),
            is_bot: false,
            reads_signed_answers: true,
        }
    }

    fn slot<'a>(admin: &'a PublicKey, token: &'a str) -> RequestSlot<'a> {
        RequestSlot {
            workspace_id: WORKSPACE,
            token,
            admin,
        }
    }

    #[test]
    fn only_the_admin_opens_a_request_and_only_for_its_invite() {
        let admin = SecretKey::generate();
        let sealed = seal_request(&invite_for(&admin, "first"), &request()).unwrap();
        let admin_id = admin.public();

        let opened = open_request(&admin, slot(&admin_id, "first"), &sealed).unwrap();
        assert_eq!(opened.name, "Bob Secretname");
        assert!(open_request(&admin, slot(&admin_id, "second"), &sealed).is_err());
        let elsewhere = RequestSlot {
            workspace_id: "other",
            ..slot(&admin_id, "first")
        };
        assert!(open_request(&admin, elsewhere, &sealed).is_err());
        let someone_else = SecretKey::generate();
        assert!(open_request(&someone_else, slot(&admin_id, "first"), &sealed).is_err());
        let naming_someone_else = someone_else.public();
        assert!(open_request(&admin, slot(&naming_someone_else, "first"), &sealed).is_err());
    }

    #[test]
    fn malformed_requests_are_refused_as_not_for_this_invite() {
        let admin = SecretKey::generate();
        let admin_id = admin.public();
        let malformed = [
            Vec::new(),
            b"junk".to_vec(),
            postcard::to_stdvec(&SealedRequest {
                kem_output: Vec::new(),
                ciphertext: Vec::new(),
            })
            .unwrap(),
            postcard::to_stdvec(&SealedRequest {
                kem_output: vec![7; 31],
                ciphertext: vec![1; 64],
            })
            .unwrap(),
            postcard::to_stdvec(&SealedRequest {
                kem_output: vec![7; 33],
                ciphertext: vec![1; 64],
            })
            .unwrap(),
        ];
        for bytes in malformed {
            let error = open_request(&admin, slot(&admin_id, "first"), &bytes).unwrap_err();
            assert!(error.downcast_ref::<NotForThisInvite>().is_some(), "{error:#}");
        }
    }

    #[test]
    fn each_invite_has_its_own_request_key() {
        let admin = SecretKey::generate();
        let first = request_key_pair(&admin, "first").unwrap().public;
        assert_eq!(first, request_key_pair(&admin, "first").unwrap().public);
        assert_ne!(first, request_key_pair(&admin, "second").unwrap().public);
        assert_ne!(
            first,
            request_key_pair(&SecretKey::generate(), "first").unwrap().public
        );
    }

    #[test]
    fn a_request_from_an_older_version_is_told_apart() {
        let admin = SecretKey::generate();
        let sealed = seal_request(&invite_for(&admin, "first"), &request()).unwrap();
        assert!(is_unsealed_request(&postcard::to_stdvec(&request()).unwrap()));
        assert!(!is_unsealed_request(&sealed));
        assert!(!is_unsealed_request(b"junk"));
    }

    #[test]
    fn a_sealed_request_doesnt_show_the_name() {
        let admin = SecretKey::generate();
        let sealed = seal_request(&invite_for(&admin, "first"), &request()).unwrap();
        let name = b"Secretname";
        assert!(!sealed.windows(name.len()).any(|window| window == name));
    }
}
