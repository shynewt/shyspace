//! Linking a new device from one the person already uses: the link code the
//! voucher shows, the request the new device seals to it, and the check
//! code both screens show before the person confirms.
//!
//! Six digits are enough because neither side picks its randomness after
//! seeing the other's: the code's token is the hash of the voucher's nonce,
//! fixed before any request exists, and the new device's one-time key is
//! fresh for every request. The nonce and the request key live only in the
//! voucher's memory, so a code allows one try even across a restart.

use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{PublicKey, SecretKey};
use openmls_rust_crypto::RustCrypto;
use openmls_traits::{crypto::OpenMlsCrypto, types::HpkeCiphertext};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::device::verify_signature;
use crate::private::crypto::{HPKE, SealingKeyPair, is_valid_sealing_public_key, new_sealing_key_pair};

const CODE_PREFIX: &str = "shyl1_";
const NONCE_CONTEXT: &str = "shyspace link nonce v1";
const REQUEST_INFO_DOMAIN: &str = "shyspace link request v1";
const HANDOVER_KEY_LABEL: &str = "shyspace link handover key v1";
const CHECK_LABEL: &str = "shyspace link check v1";
const CHECK_CODE_CONTEXT: &str = "shyspace link check code v1";
pub const LINK_CODE_LIFETIME_MS: i64 = 10 * 60 * 1000;

/// Names a link's bucket slots and commits the voucher to its nonce.
pub type Token = [u8; 16];

/// What the voucher keeps in memory for one code, and nowhere else.
pub struct LinkSecrets {
    nonce: Zeroizing<[u8; 32]>,
    request_key: SealingKeyPair,
}

impl LinkSecrets {
    pub fn new() -> Result<Self> {
        Ok(Self {
            nonce: Zeroizing::new(rand::random()),
            request_key: new_sealing_key_pair()?,
        })
    }

    pub fn token(&self) -> Token {
        token_of(&self.nonce)
    }

    pub fn request_public_key(&self) -> &[u8] {
        &self.request_key.public
    }
}

fn token_of(nonce: &[u8; 32]) -> Token {
    let digest = blake3::derive_key(NONCE_CONTEXT, nonce);
    digest[..16].try_into().expect("a 32-byte digest has 16 bytes")
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkCode {
    pub workspace_id: String,
    pub workspace_name: String,
    pub admin: PublicKey,
    /// The admin's MLS signature key, so the device can tell the Welcome
    /// is the admin's.
    pub admin_mls_key: Vec<u8>,
    pub person: PublicKey,
    /// The device that made the code; it signs the check and the hand-over.
    pub voucher: PublicKey,
    pub token: Token,
    pub expires_ms: i64,
    pub request_url: String,
    pub answer_url: String,
    pub request_key: Vec<u8>,
}

/// Never prints the presigned links, so a stray `{:?}` can't hand someone
/// the code's way into the bucket.
impl std::fmt::Debug for LinkCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LinkCode")
            .field("workspace_id", &self.workspace_id)
            .field("person", &self.person)
            .field("voucher", &self.voucher)
            .field("token", &hex::encode(self.token))
            .field("expires_ms", &self.expires_ms)
            .field("request_url", &"<hidden>")
            .field("answer_url", &"<hidden>")
            .finish()
    }
}

impl LinkCode {
    pub fn encode(&self) -> Result<String> {
        Ok(format!(
            "{CODE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(postcard::to_stdvec(self)?)
        ))
    }

    pub fn decode(code: &str) -> Result<Self> {
        let payload = code
            .trim()
            .strip_prefix(CODE_PREFIX)
            .context("not a shyspace link code")?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).context("link code is damaged")?;
        let code: Self = postcard::from_bytes(&bytes).context("link code is damaged")?;
        ensure!(
            is_valid_sealing_public_key(&code.request_key),
            "link code is damaged"
        );
        Ok(code)
    }

    pub fn is_expired(&self, now_ms: i64) -> bool {
        now_ms >= self.expires_ms
    }

    fn request_info(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&(
            REQUEST_INFO_DOMAIN,
            &self.workspace_id,
            self.token,
            self.voucher.as_bytes(),
        ))?)
    }
}

/// Only for the person's own screens ("Pixel, linked 3 Dec").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
    Android,
    Ios,
}

/// What the admin needs to add the device: phase A's request, without the
/// key it names (that is [`LinkRequest::device`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkedDevice {
    pub key_package: Vec<u8>,
    /// The device's identity-key signature over its MLS signature key.
    pub binding: Vec<u8>,
    /// The device's signature agreeing to belong to the person.
    pub acceptance: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkRequest {
    pub device: PublicKey,
    pub linked: LinkedDevice,
    /// The new device's one-time key the hand-over is sealed to.
    pub handover_key: Vec<u8>,
    /// The device's identity key over the one-time key, for this code.
    pub handover_signature: Vec<u8>,
    pub platform: Platform,
}

impl LinkRequest {
    pub fn new(
        device: &SecretKey,
        code: &LinkCode,
        linked: LinkedDevice,
        handover_key: Vec<u8>,
        platform: Platform,
    ) -> Result<Self> {
        let signed = handover_key_bytes(code, &device.public(), &handover_key)?;
        Ok(Self {
            device: device.public(),
            linked,
            handover_signature: device.sign(&signed).to_bytes().to_vec(),
            handover_key,
            platform,
        })
    }
}

fn handover_key_bytes(code: &LinkCode, device: &PublicKey, handover_key: &[u8]) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        HANDOVER_KEY_LABEL,
        &code.workspace_id,
        code.token,
        device.as_bytes(),
        handover_key,
    ))?)
}

#[derive(Serialize, Deserialize)]
struct SealedRequest {
    kem_output: Vec<u8>,
    ciphertext: Vec<u8>,
}

pub fn seal_link_request(code: &LinkCode, request: &LinkRequest) -> Result<Vec<u8>> {
    let plaintext = Zeroizing::new(postcard::to_stdvec(request)?);
    let sealed = RustCrypto::default()
        .hpke_seal(HPKE, &code.request_key, &code.request_info()?, &[], &plaintext)
        .map_err(|error| anyhow!("sealing the link request: {error:?}"))?;
    Ok(postcard::to_stdvec(&SealedRequest {
        kem_output: sealed.kem_output.as_slice().to_vec(),
        ciphertext: sealed.ciphertext.as_slice().to_vec(),
    })?)
}

/// Opens a request sealed for this code, and checks that the device signed
/// its one-time key for it. Anything else in the slot is refused.
pub fn open_link_request(code: &LinkCode, secrets: &LinkSecrets, bytes: &[u8]) -> Result<LinkRequest> {
    let sealed: SealedRequest = postcard::from_bytes(bytes).context("not a link request")?;
    let ciphertext = HpkeCiphertext {
        kem_output: sealed.kem_output.into(),
        ciphertext: sealed.ciphertext.into(),
    };
    let plaintext = RustCrypto::default()
        .hpke_open(
            HPKE,
            &ciphertext,
            &secrets.request_key.secret,
            &code.request_info()?,
            &[],
        )
        .map(Zeroizing::new)
        .map_err(|_| anyhow!("not a link request sealed for this code"))?;
    let request: LinkRequest = postcard::from_bytes(&plaintext).context("reading a link request")?;
    ensure!(
        is_valid_sealing_public_key(&request.handover_key),
        "a link request with a malformed one-time key"
    );
    let signed = handover_key_bytes(code, &request.device, &request.handover_key)?;
    verify_signature(&request.device, &signed, &request.handover_signature)
        .context("a link request whose one-time key its device didn't sign")?;
    Ok(request)
}

/// What the voucher reveals once it has opened a request: its nonce, and
/// which device and one-time key it is answering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkCheck {
    pub token: Token,
    pub device: PublicKey,
    pub handover_key: Vec<u8>,
    pub nonce: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCheck {
    check: LinkCheck,
    signature: Vec<u8>,
}

/// The voucher's check for `request`, signed, and the code its screen shows.
/// It takes the code's secrets: once the nonce is revealed, nothing is left
/// to reveal another, so a code allows one try.
pub fn make_check(
    voucher: &SecretKey,
    code: &LinkCode,
    secrets: LinkSecrets,
    request: &LinkRequest,
) -> Result<(SignedCheck, String)> {
    ensure!(
        voucher.public() == code.voucher,
        "only the code's own device can answer it"
    );
    let check = LinkCheck {
        token: code.token,
        device: request.device,
        handover_key: request.handover_key.clone(),
        nonce: *secrets.nonce,
    };
    let signature = voucher.sign(&check_bytes(code, &check)?).to_bytes().to_vec();
    let shown = check_code(code, &check.device, &check.handover_key, &check.nonce)?;
    Ok((SignedCheck { check, signature }, shown))
}

fn check_bytes(code: &LinkCode, check: &LinkCheck) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(CHECK_LABEL, &code.workspace_id, check))?)
}

/// What a check means on the new device.
#[derive(Debug, PartialEq)]
pub enum CheckReading {
    /// The code to show and compare.
    Code(String),
    /// The voucher answered another device: someone else used this code.
    SomeoneElse,
}

/// Reads the voucher's check on the new device. A check not signed by the
/// code's voucher, or whose nonce isn't the one the token commits to, is
/// refused. The code comes from this device's own keys, never from the
/// check's.
pub fn read_check(
    code: &LinkCode,
    own_device: &PublicKey,
    own_handover_key: &[u8],
    signed: &SignedCheck,
) -> Result<CheckReading> {
    let check = &signed.check;
    verify_signature(&code.voucher, &check_bytes(code, check)?, &signed.signature)
        .context("a check not signed by the device that made the code")?;
    ensure!(check.token == code.token, "a check for another code");
    ensure!(
        token_of(&check.nonce) == code.token,
        "a check whose nonce isn't the code's"
    );
    if check.device != *own_device || check.handover_key != own_handover_key {
        return Ok(CheckReading::SomeoneElse);
    }
    Ok(CheckReading::Code(check_code(
        code,
        own_device,
        own_handover_key,
        &check.nonce,
    )?))
}

/// Six digits, as "482 917".
fn check_code(code: &LinkCode, device: &PublicKey, handover_key: &[u8], nonce: &[u8; 32]) -> Result<String> {
    let bytes = postcard::to_stdvec(&(
        &code.workspace_id,
        code.token,
        code.voucher.as_bytes(),
        device.as_bytes(),
        handover_key,
        nonce,
    ))?;
    let digest = blake3::derive_key(CHECK_CODE_CONTEXT, &bytes);
    let number =
        u64::from_be_bytes(digest[..8].try_into().expect("a 32-byte digest has 8 bytes")) % 1_000_000;
    Ok(format!("{:03} {:03}", number / 1000, number % 1000))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) struct Voucher {
        pub identity: SecretKey,
        pub secrets: LinkSecrets,
        pub code: LinkCode,
    }

    pub(crate) fn voucher() -> Voucher {
        let identity = SecretKey::generate();
        let secrets = LinkSecrets::new().unwrap();
        let code = LinkCode {
            workspace_id: "acme".into(),
            workspace_name: "Acme".into(),
            admin: SecretKey::generate().public(),
            admin_mls_key: vec![1, 2, 3],
            person: identity.public(),
            voucher: identity.public(),
            token: secrets.token(),
            expires_ms: 1_000,
            request_url: "https://bucket/request".into(),
            answer_url: "https://bucket/answer".into(),
            request_key: secrets.request_public_key().to_vec(),
        };
        Voucher {
            identity,
            secrets,
            code,
        }
    }

    pub(crate) struct NewDevice {
        pub identity: SecretKey,
        pub handover: SealingKeyPair,
        pub request: LinkRequest,
    }

    pub(crate) fn new_device(code: &LinkCode) -> NewDevice {
        let identity = SecretKey::generate();
        let handover = new_sealing_key_pair().unwrap();
        let linked = LinkedDevice {
            key_package: vec![4; 40],
            binding: vec![5; 64],
            acceptance: vec![6; 64],
        };
        let request =
            LinkRequest::new(&identity, code, linked, handover.public.clone(), Platform::Linux).unwrap();
        NewDevice {
            identity,
            handover,
            request,
        }
    }

    #[test]
    fn a_link_code_round_trips_and_runs_out() {
        let code = voucher().code;
        let encoded = code.encode().unwrap();
        assert!(encoded.starts_with("shyl1_"));
        assert_eq!(LinkCode::decode(&format!("  {encoded}\n")).unwrap(), code);
        assert!(LinkCode::decode("shy3_abc").is_err());
        assert!(LinkCode::decode("shyl1_!!").is_err());
        assert!(!code.is_expired(999));
        assert!(code.is_expired(1_000));
    }

    #[test]
    fn the_token_commits_to_the_nonce_and_each_code_has_its_own() {
        let first = LinkSecrets::new().unwrap();
        let second = LinkSecrets::new().unwrap();
        assert_eq!(first.token(), token_of(&first.nonce));
        assert_ne!(first.token(), second.token());
        assert_ne!(first.request_public_key(), second.request_public_key());
    }

    #[test]
    fn both_screens_show_the_same_code_for_the_request_the_voucher_opened() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let sealed = seal_link_request(&voucher.code, &device.request).unwrap();

        let opened = open_link_request(&voucher.code, &voucher.secrets, &sealed).unwrap();
        let (check, on_voucher) =
            make_check(&voucher.identity, &voucher.code, voucher.secrets, &opened).unwrap();
        let on_device = read_check(
            &voucher.code,
            &device.identity.public(),
            &device.handover.public,
            &check,
        )
        .unwrap();

        assert_eq!(opened, device.request);
        assert_eq!(on_device, CheckReading::Code(on_voucher.clone()));
        assert_eq!(on_voucher.len(), 7);
        assert!(
            on_voucher
                .chars()
                .enumerate()
                .all(|(index, character)| match index {
                    3 => character == ' ',
                    _ => character.is_ascii_digit(),
                })
        );
    }

    #[test]
    fn a_request_opens_only_for_its_code() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let sealed = seal_link_request(&voucher.code, &device.request).unwrap();

        let other_token = LinkCode {
            token: [9; 16],
            ..voucher.code.clone()
        };
        let other_workspace = LinkCode {
            workspace_id: "other".into(),
            ..voucher.code.clone()
        };
        let other_voucher = LinkCode {
            voucher: SecretKey::generate().public(),
            ..voucher.code.clone()
        };
        for code in [other_token, other_workspace, other_voucher] {
            assert!(open_link_request(&code, &voucher.secrets, &sealed).is_err());
        }
        let other_secrets = LinkSecrets::new().unwrap();
        assert!(open_link_request(&voucher.code, &other_secrets, &sealed).is_err());
        assert!(open_link_request(&voucher.code, &voucher.secrets, b"junk").is_err());
    }

    #[test]
    fn a_request_whose_one_time_key_its_device_did_not_sign_is_refused() {
        let voucher = voucher();
        let mut device = new_device(&voucher.code);
        device.request.handover_key = new_sealing_key_pair().unwrap().public;
        let sealed = seal_link_request(&voucher.code, &device.request).unwrap();

        assert!(open_link_request(&voucher.code, &voucher.secrets, &sealed).is_err());
    }

    #[test]
    fn a_check_is_refused_unless_the_codes_voucher_signed_it_with_the_committed_nonce() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let (signed, _) =
            make_check(&voucher.identity, &voucher.code, voucher.secrets, &device.request).unwrap();
        let read = |signed: &SignedCheck| {
            read_check(
                &voucher.code,
                &device.identity.public(),
                &device.handover.public,
                signed,
            )
        };

        let mut by_someone_else = signed.clone();
        by_someone_else.signature = SecretKey::generate()
            .sign(&check_bytes(&voucher.code, &signed.check).unwrap())
            .to_bytes()
            .to_vec();
        assert!(read(&by_someone_else).is_err());

        // A fake voucher can't pick a nonce after seeing the device's keys:
        // any nonce but the committed one fails, even signed by the voucher.
        for check in [
            LinkCheck {
                nonce: [7; 32],
                ..signed.check.clone()
            },
            LinkCheck {
                token: [7; 16],
                ..signed.check.clone()
            },
        ] {
            let resigned = SignedCheck {
                signature: voucher
                    .identity
                    .sign(&check_bytes(&voucher.code, &check).unwrap())
                    .to_bytes()
                    .to_vec(),
                check,
            };
            assert!(read(&resigned).is_err());
        }
        assert!(matches!(read(&signed).unwrap(), CheckReading::Code(_)));
    }

    /// Each case needs its own code: a code's check is made once.
    #[test]
    fn a_check_answering_another_device_says_someone_else_used_the_code() {
        let answered_for: [fn(&NewDevice, &NewDevice) -> LinkRequest; 2] = [
            |_, intruder| intruder.request.clone(),
            |device, intruder| LinkRequest {
                handover_key: intruder.handover.public.clone(),
                ..device.request.clone()
            },
        ];
        for answer_for in answered_for {
            let voucher = voucher();
            let device = new_device(&voucher.code);
            let intruder = new_device(&voucher.code);
            let (check, _) = make_check(
                &voucher.identity,
                &voucher.code,
                voucher.secrets,
                &answer_for(&device, &intruder),
            )
            .unwrap();

            let reading = read_check(
                &voucher.code,
                &device.identity.public(),
                &device.handover.public,
                &check,
            )
            .unwrap();

            assert_eq!(reading, CheckReading::SomeoneElse);
        }
    }

    #[test]
    fn only_the_codes_own_device_can_make_its_check() {
        let voucher = voucher();
        let device = new_device(&voucher.code);
        let impostor = SecretKey::generate();

        assert!(make_check(&impostor, &voucher.code, voucher.secrets, &device.request).is_err());
    }
}
