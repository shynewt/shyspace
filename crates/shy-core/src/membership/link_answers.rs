//! What the admin's app writes back about a link: the welcome for a device
//! it added, or why it didn't add one. Both are signed by the admin and
//! bound to the code's token and the device, so someone with the bucket
//! keys can neither forge a refusal, which makes the new device wipe what
//! it was handed, nor move a welcome to another link.

use anyhow::{Context, Result, ensure};
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};

use super::{admin::DeviceWelcome, device::verify_signature, link::Token};

const WELCOME_LABEL: &str = "shyspace link welcome v1";
const OUTCOME_LABEL: &str = "shyspace link outcome v1";

/// Why the admin's app didn't add a device.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Reason {
    /// The certificate went stale before the admin's app saw it: the voucher
    /// vouches again, and nothing is wiped.
    Stale,
    TooManyDevices,
    DailyLimit,
    /// The person removed the device before it was added.
    Cancelled,
    /// Any other rule the certificate or request broke.
    Refused,
}

impl Reason {
    pub fn is_retry(self) -> bool {
        self == Self::Stale
    }

    /// Whether the refusal is followed by a key-only commit. A retry isn't,
    /// and neither is a link past the person's daily limit: a Link is always
    /// written by one of the person's own devices, which holds every key it
    /// could hand over, so past the limit another epoch protects nothing and
    /// costs every member one.
    pub fn needs_key_only_commit(self) -> bool {
        !matches!(self, Self::Stale | Self::DailyLimit)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Outcome {
    pub token: Token,
    pub device: PublicKey,
    /// For the voucher, which knows the certificate: a later outcome for the
    /// same code can't pass for this one.
    pub certificate_hash: [u8; 32],
    pub reason: Reason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedOutcome {
    outcome: Outcome,
    signature: Vec<u8>,
}

fn outcome_bytes(workspace_id: &str, outcome: &Outcome) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(OUTCOME_LABEL, workspace_id, outcome))?)
}

impl SignedOutcome {
    pub fn sign(admin: &SecretKey, workspace_id: &str, outcome: Outcome) -> Result<Self> {
        let signature = admin
            .sign(&outcome_bytes(workspace_id, &outcome)?)
            .to_bytes()
            .to_vec();
        Ok(Self { outcome, signature })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes).context("not a link outcome")
    }

    /// The outcome, if the admin signed it for this code and this device.
    pub fn read(
        &self,
        admin: &PublicKey,
        workspace_id: &str,
        token: &Token,
        device: &PublicKey,
    ) -> Result<&Outcome> {
        verify_signature(
            admin,
            &outcome_bytes(workspace_id, &self.outcome)?,
            &self.signature,
        )
        .context("a link outcome the admin didn't sign")?;
        ensure!(
            self.outcome.token == *token && self.outcome.device == *device,
            "a link outcome for another code or device"
        );
        Ok(&self.outcome)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedWelcome {
    welcome: Vec<u8>,
    sealed_grant: Vec<u8>,
    map_digest: [u8; 32],
    signature: Vec<u8>,
}

fn welcome_bytes(
    workspace_id: &str,
    token: &Token,
    device: &PublicKey,
    welcome: &DeviceWelcome,
) -> Result<Vec<u8>> {
    Ok(postcard::to_stdvec(&(
        WELCOME_LABEL,
        workspace_id,
        token,
        device.as_bytes(),
        welcome.map_digest,
        blake3::hash(&welcome.welcome).as_bytes(),
        blake3::hash(&welcome.sealed_grant).as_bytes(),
    ))?)
}

impl SignedWelcome {
    pub fn sign(
        admin: &SecretKey,
        workspace_id: &str,
        token: &Token,
        device: &PublicKey,
        welcome: &DeviceWelcome,
    ) -> Result<Self> {
        let signature = admin
            .sign(&welcome_bytes(workspace_id, token, device, welcome)?)
            .to_bytes()
            .to_vec();
        Ok(Self {
            welcome: welcome.welcome.clone(),
            sealed_grant: welcome.sealed_grant.clone(),
            map_digest: welcome.map_digest,
            signature,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(self)?)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        postcard::from_bytes(bytes).context("not a link welcome")
    }

    /// The welcome, if the admin signed it for this code and this device.
    pub fn read(
        &self,
        admin: &PublicKey,
        workspace_id: &str,
        token: &Token,
        device: &PublicKey,
    ) -> Result<DeviceWelcome> {
        let welcome = DeviceWelcome {
            welcome: self.welcome.clone(),
            sealed_grant: self.sealed_grant.clone(),
            map_digest: self.map_digest,
        };
        verify_signature(
            admin,
            &welcome_bytes(workspace_id, token, device, &welcome)?,
            &self.signature,
        )
        .context("a link welcome the admin didn't sign for this device")?;
        Ok(welcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = "acme";

    fn outcome(device: PublicKey) -> Outcome {
        Outcome {
            token: [1; 16],
            device,
            certificate_hash: [2; 32],
            reason: Reason::TooManyDevices,
        }
    }

    fn welcome() -> DeviceWelcome {
        DeviceWelcome {
            welcome: vec![3; 50],
            sealed_grant: vec![4; 80],
            map_digest: [5; 32],
        }
    }

    #[test]
    fn an_outcome_counts_only_signed_by_the_admin_for_its_code_and_device() {
        let admin = SecretKey::generate();
        let device = SecretKey::generate().public();
        let signed = SignedOutcome::sign(&admin, WORKSPACE, outcome(device)).unwrap();
        let read = SignedOutcome::decode(&signed.encode().unwrap()).unwrap();

        assert_eq!(
            read.read(&admin.public(), WORKSPACE, &[1; 16], &device).unwrap(),
            &outcome(device)
        );
        assert!(
            read.read(&SecretKey::generate().public(), WORKSPACE, &[1; 16], &device)
                .is_err()
        );
        assert!(read.read(&admin.public(), "other", &[1; 16], &device).is_err());
        assert!(read.read(&admin.public(), WORKSPACE, &[9; 16], &device).is_err());
        assert!(
            read.read(
                &admin.public(),
                WORKSPACE,
                &[1; 16],
                &SecretKey::generate().public()
            )
            .is_err()
        );
        let forged = SignedOutcome::sign(&SecretKey::generate(), WORKSPACE, outcome(device)).unwrap();
        assert!(
            forged
                .read(&admin.public(), WORKSPACE, &[1; 16], &device)
                .is_err()
        );
        assert!(SignedOutcome::decode(b"junk").is_err());
    }

    #[test]
    fn a_welcome_opens_only_signed_by_the_admin_for_its_code_and_device() {
        let admin = SecretKey::generate();
        let device = SecretKey::generate().public();
        let signed = SignedWelcome::sign(&admin, WORKSPACE, &[1; 16], &device, &welcome()).unwrap();
        let read = SignedWelcome::decode(&signed.encode().unwrap()).unwrap();

        let opened = read.read(&admin.public(), WORKSPACE, &[1; 16], &device).unwrap();
        assert_eq!(
            (opened.welcome, opened.sealed_grant, opened.map_digest),
            (vec![3; 50], vec![4; 80], [5; 32])
        );
        assert!(read.read(&admin.public(), WORKSPACE, &[9; 16], &device).is_err());
        assert!(
            read.read(
                &admin.public(),
                WORKSPACE,
                &[1; 16],
                &SecretKey::generate().public()
            )
            .is_err()
        );
        let mut swapped_grant = signed.clone();
        swapped_grant.sealed_grant = vec![6; 80];
        assert!(
            swapped_grant
                .read(&admin.public(), WORKSPACE, &[1; 16], &device)
                .is_err()
        );
        assert!(SignedWelcome::decode(b"junk").is_err());
    }

    #[test]
    fn neither_a_retry_nor_a_link_past_the_daily_limit_needs_a_key_only_commit() {
        assert!(!Reason::Stale.needs_key_only_commit());
        assert!(!Reason::DailyLimit.needs_key_only_commit());
        for reason in [Reason::TooManyDevices, Reason::Cancelled, Reason::Refused] {
            assert!(reason.needs_key_only_commit());
        }
    }

    #[test]
    fn only_a_stale_certificate_is_a_retry() {
        assert!(Reason::Stale.is_retry());
        for reason in [
            Reason::TooManyDevices,
            Reason::DailyLimit,
            Reason::Cancelled,
            Reason::Refused,
        ] {
            assert!(!reason.is_retry());
        }
    }
}
