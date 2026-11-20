//! Encryption for private channels and DMs. Content is sealed under a
//! channel key and opens only in the op slot it was written for. Channel keys
//! reach members wrapped with HPKE to each member's sealing key, bound to the
//! workspace, the granter and the recipient.

use anyhow::{Context, Result, anyhow, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use iroh::PublicKey;
use openmls_rust_crypto::RustCrypto;
use openmls_traits::{
    crypto::OpenMlsCrypto,
    types::{HpkeAeadType, HpkeCiphertext, HpkeConfig, HpkeKdfType, HpkeKemType},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const CHANNEL_KEY_LEN: usize = 32;
pub type ChannelKey = [u8; CHANNEL_KEY_LEN];
pub type KeyHint = [u8; 8];
const SEALING_PUBLIC_KEY_LEN: usize = 32;

const CONTENT_KEY_CONTEXT: &str = "shyspace private content v1";
const HINT_KEY_CONTEXT: &str = "shyspace private hint v1";
const GRANT_INFO_DOMAIN: &str = "shyspace key grant v1";
const HPKE: HpkeConfig = HpkeConfig(
    HpkeKemType::DhKem25519,
    HpkeKdfType::HkdfSha256,
    HpkeAeadType::ChaCha20Poly1305,
);

/// The op a payload is sealed into: its workspace, author and sequence.
#[derive(Clone, Copy)]
pub struct Slot<'a> {
    pub workspace_id: &'a str,
    pub author: &'a PublicKey,
    pub seq: u64,
}

impl Slot<'_> {
    fn bytes(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&(
            CONTENT_KEY_CONTEXT,
            self.workspace_id,
            self.author.as_bytes(),
            self.seq,
        ))?)
    }
}

/// Content only a channel's members can read. The hint tells members which
/// of their keys to try; it differs for every slot, so outsiders can't tell
/// which payloads belong to the same channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedPayload {
    pub hint: KeyHint,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

pub fn new_channel_key() -> ChannelKey {
    rand::random()
}

pub fn key_hint(key: &ChannelKey, slot: Slot<'_>) -> Result<KeyHint> {
    let hint_key = blake3::derive_key(HINT_KEY_CONTEXT, key);
    let hash = blake3::keyed_hash(&hint_key, &slot.bytes()?);
    let mut hint = KeyHint::default();
    let length = hint.len();
    hint.copy_from_slice(&hash.as_bytes()[..length]);
    Ok(hint)
}

pub fn seal(key: &ChannelKey, slot: Slot<'_>, plaintext: &[u8]) -> Result<SealedPayload> {
    let content_key = blake3::derive_key(CONTENT_KEY_CONTEXT, key);
    let nonce = XNonce::generate();
    let ciphertext = XChaCha20Poly1305::new(&content_key.into())
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: &slot.bytes()?,
            },
        )
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(SealedPayload {
        hint: key_hint(key, slot)?,
        nonce: nonce.into(),
        ciphertext,
    })
}

/// Fails for the wrong key or a payload copied from another slot.
pub fn open(key: &ChannelKey, slot: Slot<'_>, payload: &SealedPayload) -> Result<Vec<u8>> {
    let content_key = blake3::derive_key(CONTENT_KEY_CONTEXT, key);
    XChaCha20Poly1305::new(&content_key.into())
        .decrypt(
            &payload.nonce.into(),
            Payload {
                msg: &payload.ciphertext,
                aad: &slot.bytes()?,
            },
        )
        .map_err(|_| anyhow!("sealed payload doesn't open with this key here"))
}

/// A member's key for receiving channel keys. Separate from their identity
/// and MLS keys, so no key does two jobs.
pub struct SealingKeyPair {
    pub secret: Zeroizing<Vec<u8>>,
    pub public: Vec<u8>,
}

pub fn new_sealing_key_pair() -> Result<SealingKeyPair> {
    let seed = Zeroizing::new(rand::random::<[u8; 32]>());
    let pair = RustCrypto::default()
        .derive_hpke_keypair(HPKE, seed.as_slice())
        .map_err(|error| anyhow!("making a sealing key: {error:?}"))?;
    Ok(SealingKeyPair {
        secret: Zeroizing::new(pair.private.to_vec()),
        public: pair.public,
    })
}

pub fn is_valid_sealing_public_key(public: &[u8]) -> bool {
    public.len() == SEALING_PUBLIC_KEY_LEN
}

/// One channel key wrapped for one recipient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wrap {
    pub kem_output: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Who hands out a wrap, in which workspace.
#[derive(Clone, Copy)]
pub struct Granting<'a> {
    pub workspace_id: &'a str,
    pub granter: &'a PublicKey,
}

impl Granting<'_> {
    fn info(&self, recipient_public: &[u8]) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&(
            GRANT_INFO_DOMAIN,
            self.workspace_id,
            self.granter.as_bytes(),
            recipient_public,
        ))?)
    }
}

pub fn wrap(granting: Granting<'_>, recipient_public: &[u8], plaintext: &[u8]) -> Result<Wrap> {
    ensure!(
        is_valid_sealing_public_key(recipient_public),
        "malformed sealing key"
    );
    let sealed = RustCrypto::default()
        .hpke_seal(
            HPKE,
            recipient_public,
            &granting.info(recipient_public)?,
            &[],
            plaintext,
        )
        .map_err(|error| anyhow!("wrapping a channel key: {error:?}"))?;
    Ok(Wrap {
        kem_output: sealed.kem_output.as_slice().to_vec(),
        ciphertext: sealed.ciphertext.as_slice().to_vec(),
    })
}

/// Fails when the wrap is for someone else, or was made by another granter
/// or in another workspace. Devices try every wrap, so failing is normal.
pub fn unwrap(granting: Granting<'_>, own: &SealingKeyPair, wrap: &Wrap) -> Result<Zeroizing<Vec<u8>>> {
    let ciphertext = HpkeCiphertext {
        kem_output: wrap.kem_output.clone().into(),
        ciphertext: wrap.ciphertext.clone().into(),
    };
    RustCrypto::default()
        .hpke_open(HPKE, &ciphertext, &own.secret, &granting.info(&own.public)?, &[])
        .map(Zeroizing::new)
        .map_err(|_| anyhow!("wrap isn't for this key"))
        .context("unwrapping a channel key")
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    #[test]
    fn a_payload_opens_only_with_its_key_in_its_slot() {
        let author = SecretKey::generate().public();
        let slot = Slot {
            workspace_id: "w",
            author: &author,
            seq: 7,
        };
        let key = new_channel_key();
        let sealed = seal(&key, slot, b"secret").unwrap();
        assert_eq!(open(&key, slot, &sealed).unwrap(), b"secret");
        assert_eq!(sealed.hint, key_hint(&key, slot).unwrap());

        assert!(open(&new_channel_key(), slot, &sealed).is_err());
        let other_author = SecretKey::generate().public();
        for copied_to in [
            Slot { seq: 8, ..slot },
            Slot {
                author: &other_author,
                ..slot
            },
            Slot {
                workspace_id: "other",
                ..slot
            },
        ] {
            assert!(open(&key, copied_to, &sealed).is_err());
        }
    }

    #[test]
    fn hints_differ_per_slot_so_payloads_cannot_be_grouped() {
        let author = SecretKey::generate().public();
        let key = new_channel_key();
        let hint = |seq| {
            key_hint(
                &key,
                Slot {
                    workspace_id: "w",
                    author: &author,
                    seq,
                },
            )
            .unwrap()
        };
        assert_ne!(hint(1), hint(2));
    }

    #[test]
    fn a_wrap_opens_only_for_its_recipient_from_its_granter() {
        let granter = SecretKey::generate().public();
        let granting = Granting {
            workspace_id: "w",
            granter: &granter,
        };
        let bob = new_sealing_key_pair().unwrap();
        let carol = new_sealing_key_pair().unwrap();
        let wrapped = wrap(granting, &bob.public, b"channel key").unwrap();
        assert_eq!(
            unwrap(granting, &bob, &wrapped).unwrap().as_slice(),
            b"channel key"
        );
        assert!(unwrap(granting, &carol, &wrapped).is_err());

        let mallory = SecretKey::generate().public();
        let copied_by_mallory = Granting {
            granter: &mallory,
            ..granting
        };
        assert!(unwrap(copied_by_mallory, &bob, &wrapped).is_err());
        let other_workspace = Granting {
            workspace_id: "other",
            ..granting
        };
        assert!(unwrap(other_workspace, &bob, &wrapped).is_err());
    }

    #[test]
    fn malformed_sealing_keys_are_refused() {
        let granter = SecretKey::generate().public();
        let granting = Granting {
            workspace_id: "w",
            granter: &granter,
        };
        assert!(wrap(granting, &[1, 2, 3], b"key").is_err());
    }
}
