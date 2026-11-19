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

fn header_bytes(author: &PublicKey, seq: u64, epoch: u64) -> Vec<u8> {
    let mut header = Vec::with_capacity(48);
    header.extend_from_slice(author.as_bytes());
    header.extend_from_slice(&seq.to_be_bytes());
    header.extend_from_slice(&epoch.to_be_bytes());
    header
}

/// Sign-then-encrypt. The signature covers the cleartext header too, so an
/// envelope cannot be replayed under another author, sequence or epoch.
pub fn seal(
    identity: &SecretKey,
    epoch_key: &EpochKey,
    epoch: u64,
    seq: u64,
    body: &OpBody,
) -> Result<Envelope> {
    let author = identity.public();
    let header = header_bytes(&author, seq, epoch);
    let body_bytes = postcard::to_stdvec(body)?;
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

pub fn open(envelope: &Envelope, epoch_key: &EpochKey) -> Result<VerifiedOp> {
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
    let body: OpBody = postcard::from_bytes(&signed.body)?;
    ensure!(body.lamport > 0, "lamport clock must start at 1");
    Ok(VerifiedOp {
        author: envelope.author,
        seq: envelope.seq,
        body,
    })
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
