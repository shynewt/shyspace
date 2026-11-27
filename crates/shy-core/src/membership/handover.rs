//! What the voucher hands a new device once the person has confirmed the
//! check code: the content of a newcomer's grant and nothing more, so the
//! device can read at once. It is sealed to the one-time key the confirmed
//! device signed for this code and signed by the voucher, and travels in
//! the code's answer slot after the check.

use anyhow::{Context, Result, anyhow, ensure};
use iroh::{PublicKey, SecretKey};
use openmls_rust_crypto::RustCrypto;
use openmls_traits::{crypto::OpenMlsCrypto, types::HpkeCiphertext};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{
    device::{BoundFreeze, DeviceRecord, verify_signature},
    keyring::KeyHistory,
    link::{LinkCode, LinkRequest, SignedCheck, Token},
};
use crate::{private::crypto::HPKE, transport::bucket::BucketConfig};

const HANDOVER_INFO_DOMAIN: &str = "shyspace link handover v1";
const HANDOVER_SIGNATURE_LABEL: &str = "shyspace link handover signature v1";

#[derive(Serialize, Deserialize)]
pub(crate) struct Handover {
    pub voucher: PublicKey,
    pub device: PublicKey,
    pub token: Token,
    /// The voucher's epoch when it sealed, which is its certificate's: no
    /// newer keys are handed over.
    pub epoch: u64,
    pub history: KeyHistory,
    pub devices: Vec<DeviceRecord>,
    pub bound_frozen: Vec<BoundFreeze>,
    pub bucket: BucketConfig,
    /// When `bucket` was last changed, as in a grant.
    pub bucket_lamport: u64,
}

#[derive(Serialize, Deserialize)]
pub struct SealedHandover {
    kem_output: Vec<u8>,
    ciphertext: Vec<u8>,
    signature: Vec<u8>,
}

/// What the voucher writes to a code's answer slot: first the check, then
/// the hand-over in its place.
#[derive(Serialize, Deserialize)]
pub enum Answer {
    Check(SignedCheck),
    Handover(SealedHandover),
}

impl Answer {
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
    }

    /// Anything that isn't an answer (junk someone with the bucket keys put
    /// there) is an error, which the new device ignores and keeps waiting.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes).context("not a link answer")
    }
}

fn handover_info(code: &LinkCode, device: &PublicKey) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        HANDOVER_INFO_DOMAIN,
        &code.workspace_id,
        code.token,
        code.voucher.as_bytes(),
        device.as_bytes(),
    ))?)
}

fn signed_bytes(info: &[u8], kem_output: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        HANDOVER_SIGNATURE_LABEL,
        info,
        kem_output,
        ciphertext,
    ))?)
}

/// Seals `handover` for the device that made `request`, which the person
/// confirmed. Refuses one that names another voucher, device or code.
pub(crate) fn seal_handover(
    voucher: &SecretKey,
    code: &LinkCode,
    request: &LinkRequest,
    handover: &Handover,
) -> Result<SealedHandover> {
    ensure!(
        voucher.public() == code.voucher && handover.voucher == code.voucher,
        "only the code's own device can hand over"
    );
    ensure!(
        handover.device == request.device && handover.token == code.token,
        "a hand-over for another device or code"
    );
    let info = handover_info(code, &request.device)?;
    let plaintext = Zeroizing::new(postcard::to_stdvec(handover)?);
    let sealed = RustCrypto::default()
        .hpke_seal(HPKE, &request.handover_key, &info, &[], &plaintext)
        .map_err(|error| anyhow!("sealing the hand-over: {error:?}"))?;
    let kem_output = sealed.kem_output.as_slice().to_vec();
    let ciphertext = sealed.ciphertext.as_slice().to_vec();
    let signature = voucher
        .sign(&signed_bytes(&info, &kem_output, &ciphertext)?)
        .to_bytes()
        .to_vec();
    Ok(SealedHandover {
        kem_output,
        ciphertext,
        signature,
    })
}

/// Opens the hand-over on the new device: signed by the code's voucher,
/// sealed to this device's one-time key, for this device and code.
pub(crate) fn open_handover(
    code: &LinkCode,
    own_device: &PublicKey,
    own_handover_secret: &[u8],
    sealed: &SealedHandover,
) -> Result<Handover> {
    let info = handover_info(code, own_device)?;
    verify_signature(
        &code.voucher,
        &signed_bytes(&info, &sealed.kem_output, &sealed.ciphertext)?,
        &sealed.signature,
    )
    .context("a hand-over not signed by the device that made the code")?;
    let ciphertext = HpkeCiphertext {
        kem_output: sealed.kem_output.clone().into(),
        ciphertext: sealed.ciphertext.clone().into(),
    };
    let plaintext = RustCrypto::default()
        .hpke_open(HPKE, &ciphertext, own_handover_secret, &info, &[])
        .map(Zeroizing::new)
        .map_err(|_| anyhow!("a hand-over sealed for another device"))?;
    let handover: Handover = postcard::from_bytes(&plaintext).context("reading the hand-over")?;
    ensure!(
        handover.voucher == code.voucher && handover.device == *own_device && handover.token == code.token,
        "a hand-over for another device or code"
    );
    Ok(handover)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::membership::link::{
        make_check,
        tests::{new_device, voucher},
    };

    fn handover_for(code: &LinkCode, device: &PublicKey) -> Handover {
        Handover {
            voucher: code.voucher,
            device: *device,
            token: code.token,
            epoch: 3,
            history: KeyHistory {
                epochs: Vec::new(),
                frozen: Vec::new(),
            },
            devices: Vec::new(),
            bound_frozen: Vec::new(),
            bucket: BucketConfig {
                endpoint: "https://s3.example".into(),
                region: "eu".into(),
                bucket: "acme".into(),
                access_key_id: "id".into(),
                secret_access_key: "secret".into(),
            },
            bucket_lamport: 7,
        }
    }

    #[test]
    fn the_confirmed_device_opens_the_handover_from_the_answer_slot() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let handover = handover_for(&voucher.code, &device.request.device);
        let sealed = seal_handover(&voucher.identity, &voucher.code, &device.request, &handover).unwrap();
        let slot = Answer::Handover(sealed).encode().unwrap();

        let Answer::Handover(read) = Answer::decode(&slot).unwrap() else {
            panic!("the slot holds the hand-over");
        };
        let opened = open_handover(
            &voucher.code,
            &device.identity.public(),
            &device.handover.secret,
            &read,
        )
        .unwrap();

        assert_eq!((opened.epoch, opened.bucket_lamport), (3, 7));
        assert_eq!(opened.bucket, handover.bucket);
    }

    #[test]
    fn the_answer_slot_carries_the_check_first_and_junk_is_no_answer() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let (check, _) =
            make_check(&voucher.identity, &voucher.code, voucher.secrets, &device.request).unwrap();

        let slot = Answer::Check(check).encode().unwrap();

        assert!(matches!(Answer::decode(&slot).unwrap(), Answer::Check(_)));
        assert!(Answer::decode(b"junk").is_err());
    }

    #[test]
    fn a_handover_opens_only_for_its_device_its_code_and_its_voucher() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let handover = handover_for(&voucher.code, &device.request.device);
        let sealed = seal_handover(&voucher.identity, &voucher.code, &device.request, &handover).unwrap();
        let open = |code: &LinkCode, own: &PublicKey, secret: &[u8], sealed: &SealedHandover| {
            open_handover(code, own, secret, sealed)
        };
        let own = device.identity.public();

        let other_code = LinkCode {
            token: [9; 16],
            ..voucher.code.clone()
        };
        assert!(open(&other_code, &own, &device.handover.secret, &sealed).is_err());
        let intruder = new_device(&voucher.code);
        assert!(
            open(
                &voucher.code,
                &intruder.identity.public(),
                &intruder.handover.secret,
                &sealed
            )
            .is_err()
        );
        assert!(open(&voucher.code, &own, &intruder.handover.secret, &sealed).is_err());

        let mut resigned_by_another =
            seal_handover(&voucher.identity, &voucher.code, &device.request, &handover).unwrap();
        let info = handover_info(&voucher.code, &own).unwrap();
        resigned_by_another.signature = SecretKey::generate()
            .sign(
                &signed_bytes(
                    &info,
                    &resigned_by_another.kem_output,
                    &resigned_by_another.ciphertext,
                )
                .unwrap(),
            )
            .to_bytes()
            .to_vec();
        assert!(open(&voucher.code, &own, &device.handover.secret, &resigned_by_another).is_err());
        assert!(open(&voucher.code, &own, &device.handover.secret, &sealed).is_ok());
    }

    #[test]
    fn a_handover_naming_another_device_or_code_is_never_sealed() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let intruder = new_device(&voucher.code);
        for handover in [
            handover_for(&voucher.code, &intruder.request.device),
            Handover {
                token: [9; 16],
                ..handover_for(&voucher.code, &device.request.device)
            },
        ] {
            assert!(seal_handover(&voucher.identity, &voucher.code, &device.request, &handover).is_err());
        }
        let impostor = SecretKey::generate();
        let handover = handover_for(&voucher.code, &device.request.device);
        assert!(seal_handover(&impostor, &voucher.code, &device.request, &handover).is_err());
    }

    /// What a changed voucher app could seal, skipping `seal_handover`'s
    /// checks.
    fn seal_unchecked(
        voucher: &SecretKey,
        code: &LinkCode,
        request: &LinkRequest,
        handover: &Handover,
    ) -> SealedHandover {
        let info = handover_info(code, &request.device).unwrap();
        let sealed = RustCrypto::default()
            .hpke_seal(
                HPKE,
                &request.handover_key,
                &info,
                &[],
                &postcard::to_stdvec(handover).unwrap(),
            )
            .unwrap();
        let kem_output = sealed.kem_output.as_slice().to_vec();
        let ciphertext = sealed.ciphertext.as_slice().to_vec();
        let signature = voucher
            .sign(&signed_bytes(&info, &kem_output, &ciphertext).unwrap())
            .to_bytes()
            .to_vec();
        SealedHandover {
            kem_output,
            ciphertext,
            signature,
        }
    }

    #[test]
    fn a_handover_whose_contents_name_another_device_or_code_is_refused() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let own = device.identity.public();
        for handover in [
            handover_for(&voucher.code, &SecretKey::generate().public()),
            Handover {
                token: [9; 16],
                ..handover_for(&voucher.code, &own)
            },
            Handover {
                voucher: SecretKey::generate().public(),
                ..handover_for(&voucher.code, &own)
            },
        ] {
            let sealed = seal_unchecked(&voucher.identity, &voucher.code, &device.request, &handover);
            assert!(open_handover(&voucher.code, &own, &device.handover.secret, &sealed).is_err());
        }
    }

    /// Adding a field to the hand-over hands over more: this test must
    /// change with it, so the change is seen.
    #[test]
    fn the_handover_holds_exactly_a_newcomers_grant() {
        let voucher = voucher();
        let Handover {
            voucher: _,
            device: _,
            token: _,
            epoch: _,
            history: KeyHistory { epochs: _, frozen: _ },
            devices: _,
            bound_frozen: _,
            bucket: _,
            bucket_lamport: _,
        } = handover_for(&voucher.code, &voucher.code.voucher);
    }
}
