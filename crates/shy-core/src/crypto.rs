use anyhow::{Context, Result, anyhow, ensure};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit, Payload},
};
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};

use crate::ops::{Envelope, OpBody, VerifiedOp};

pub const EPOCH_KEY_LEN: usize = 32;
pub type EpochKey = [u8; EPOCH_KEY_LEN];

#[derive(Serialize, Deserialize)]
struct SignedBody {
    body: Vec<u8>,
    signature: Vec<u8>,
}

pub(crate) fn header_bytes(author: &PublicKey, seq: u64, epoch: u64) -> Vec<u8> {
    let mut header = Vec::with_capacity(48);
    header.extend_from_slice(author.as_bytes());
    header.extend_from_slice(&seq.to_be_bytes());
    header.extend_from_slice(&epoch.to_be_bytes());
    header
}

/// A validly signed op this version can't read: a teammate's newer version
/// added a kind of change. It should wait, not be dropped, so it applies
/// once this app is updated.
#[derive(Debug)]
pub struct NewerFormat;

impl std::fmt::Display for NewerFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a teammate uses a newer version of shyspace; update to see all their changes")
    }
}

impl std::error::Error for NewerFormat {}

/// Sign-then-encrypt. The signature covers the cleartext header too, so an
/// envelope cannot be replayed under another author, sequence or epoch.
pub fn seal(
    identity: &SecretKey,
    epoch_key: &EpochKey,
    epoch: u64,
    seq: u64,
    body: &OpBody,
) -> Result<Envelope> {
    seal_bytes(identity, epoch_key, epoch, seq, postcard::to_stdvec(body)?)
}

pub(crate) fn seal_bytes(
    identity: &SecretKey,
    epoch_key: &EpochKey,
    epoch: u64,
    seq: u64,
    body_bytes: Vec<u8>,
) -> Result<Envelope> {
    let author = identity.public();
    let header = header_bytes(&author, seq, epoch);
    let mut signed_message = header.clone();
    signed_message.extend_from_slice(&body_bytes);
    let signature = identity.sign(&signed_message);
    let plaintext = postcard::to_stdvec(&SignedBody {
        body: body_bytes,
        signature: signature.to_bytes().to_vec(),
    })?;

    let nonce = XNonce::generate();
    let ciphertext = XChaCha20Poly1305::new(epoch_key.into())
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: &header,
            },
        )
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(Envelope {
        author,
        seq,
        epoch,
        nonce: nonce.into(),
        ciphertext,
    })
}

/// Opens an op arriving from someone else.
pub fn open(envelope: &Envelope, epoch_key: &EpochKey) -> Result<VerifiedOp> {
    parse_body(envelope, open_body(envelope, epoch_key, Encoding::Exact)?)
}

/// Opens an op already in our log. Versions before the exact-encoding rule
/// may have kept one padded within its size limit.
pub fn open_kept(envelope: &Envelope, epoch_key: &EpochKey) -> Result<VerifiedOp> {
    parse_body(envelope, open_body(envelope, epoch_key, Encoding::Any)?)
}

/// What `envelope`'s author signed, unparsed, so ops of kinds a newer
/// version added can be compared too. Padding doesn't change it.
pub fn signed_body(envelope: &Envelope, epoch_key: &EpochKey) -> Result<Vec<u8>> {
    open_body(envelope, epoch_key, Encoding::Any)
}

fn parse_body(envelope: &Envelope, body_bytes: Vec<u8>) -> Result<VerifiedOp> {
    let body: OpBody = postcard::from_bytes(&body_bytes).map_err(|_| anyhow::Error::new(NewerFormat))?;
    ensure!(body.lamport > 0, "lamport clock must start at 1");
    Ok(VerifiedOp {
        author: envelope.author,
        seq: envelope.seq,
        epoch: envelope.epoch,
        body,
    })
}

enum Encoding {
    /// Only the exact encoding of what the author signed. The encryption
    /// isn't signed, so otherwise anyone with the epoch key could pad an op
    /// past its size limit to get it refused.
    Exact,
    Any,
}

/// Decrypts `envelope` and checks its signature, returning the signed body.
fn open_body(envelope: &Envelope, epoch_key: &EpochKey, encoding: Encoding) -> Result<Vec<u8>> {
    let header = header_bytes(&envelope.author, envelope.seq, envelope.epoch);
    let plaintext = XChaCha20Poly1305::new(epoch_key.into())
        .decrypt(
            &envelope.nonce.into(),
            Payload {
                msg: &envelope.ciphertext,
                aad: &header,
            },
        )
        .map_err(|_| anyhow!("decryption failed for {}:{}", envelope.author, envelope.seq))?;
    let signed: SignedBody = postcard::from_bytes(&plaintext)?;
    if matches!(encoding, Encoding::Exact) {
        ensure!(
            postcard::to_stdvec(&signed)? == plaintext,
            "padded op from {}:{}",
            envelope.author,
            envelope.seq
        );
    }
    let signature_bytes: [u8; 64] = signed
        .signature
        .as_slice()
        .try_into()
        .context("malformed signature")?;
    let mut signed_message = header;
    signed_message.extend_from_slice(&signed.body);
    envelope
        .author
        .verify(&signed_message, &Signature::from_bytes(&signature_bytes))
        .context("bad signature")?;
    Ok(signed.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Op;

    fn sample_body() -> OpBody {
        OpBody {
            lamport: 1,
            timestamp_ms: 0,
            op: Op::Profile {
                name: "Ada".into(),
                is_bot: false,
            },
        }
    }

    #[test]
    fn round_trip_verifies_author() {
        let identity = SecretKey::generate();
        let key = [7u8; 32];
        let envelope = seal(&identity, &key, 3, 1, &sample_body()).unwrap();
        let opened = open(&envelope, &key).unwrap();
        assert_eq!(opened.author, identity.public());
        assert_eq!(opened.body.op, sample_body().op);
    }

    #[test]
    fn rejects_wrong_key_and_tampered_header() {
        let identity = SecretKey::generate();
        let key = [7u8; 32];
        let envelope = seal(&identity, &key, 3, 1, &sample_body()).unwrap();
        assert!(open(&envelope, &[8u8; 32]).is_err());

        let mut replayed = envelope.clone();
        replayed.seq = 2;
        assert!(open(&replayed, &key).is_err());

        let mut impersonated = envelope;
        impersonated.author = SecretKey::generate().public();
        assert!(open(&impersonated, &key).is_err());
    }
}
